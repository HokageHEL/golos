import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { isSupportedAudio, storagePathFor } from '@/lib/jobs/media';
import { anonKey } from '@/lib/quota/identity';
import { claimAnonTrial } from '@/lib/quota/anon';
import { redisStore } from '@/lib/quota/redis';
import { verifyTurnstile } from '@/lib/turnstile';
import { currentUser } from '@/lib/auth';
import { ANON_TRIAL_SECONDS } from '@/lib/billing/pricing';

export async function POST(request: Request) {
  const body = (await request.json()) as {
    filename: string;
    mimeType: string;
    fingerprint: string;
    turnstileToken: string | null;
  };

  if (!isSupportedAudio(body.mimeType)) {
    return NextResponse.json({ error: 'unsupported_format' }, { status: 415 });
  }

  const ip = request.headers.get('x-forwarded-for')?.split(',')[0].trim() ?? '0.0.0.0';
  const user = await currentUser();
  const db = serviceClient();

  let maxDurationSeconds: number | null = null;
  let anon: string | null = null;

  if (user) {
    const { data: account } = await db
      .from('accounts')
      .select('balance_cents, minutes_included, minutes_used')
      .eq('user_id', user.id)
      .single();

    const hasPlanMinutes = (account?.minutes_included ?? 0) > (account?.minutes_used ?? 0);
    if (!hasPlanMinutes && (account?.balance_cents ?? 0) <= 0) {
      return NextResponse.json({ error: 'insufficient_balance' }, { status: 402 });
    }
  } else {
    anon = anonKey(ip, body.fingerprint);

    // Redis holds the only record of who has already used their free trial.
    // If it is unreachable we cannot tell a first-time visitor from a repeat
    // one, so we refuse rather than hand out unlimited free transcriptions.
    // Same fail-closed reasoning as verifyTurnstile, and for the same reason:
    // an unhandled rejection here turns an outage into a 500.
    let claim;
    try {
      claim = await claimAnonTrial({
        key: anon,
        store: redisStore(),
        verifyBot: () => verifyTurnstile(body.turnstileToken, ip),
      });
    } catch {
      return NextResponse.json({ error: 'trial_unavailable' }, { status: 503 });
    }

    if (!claim.granted) {
      return NextResponse.json({ error: claim.reason }, { status: 403 });
    }
    maxDurationSeconds = ANON_TRIAL_SECONDS;
  }

  const { data: job, error } = await db
    .from('jobs')
    .insert({
      user_id: user?.id ?? null,
      anon_key: anon,
      filename: body.filename,
      storage_path: '',
      status: 'awaiting_upload',
    })
    .select('id')
    .single();

  if (error || !job) {
    return NextResponse.json({ error: 'generic' }, { status: 500 });
  }

  const path = storagePathFor(job.id, body.filename);
  const { error: updateError } = await db
    .from('jobs')
    .update({ storage_path: path })
    .eq('id', job.id);

  if (updateError) {
    return NextResponse.json({ error: 'generic' }, { status: 500 });
  }

  const { data: signed, error: signError } = await db.storage
    .from('audio')
    .createSignedUploadUrl(path);

  if (signError || !signed) {
    return NextResponse.json({ error: 'generic' }, { status: 500 });
  }

  const response = NextResponse.json({
    jobId: job.id,
    uploadUrl: signed.signedUrl,
    uploadToken: signed.token,
    path,
    maxDurationSeconds,
  });

  if (anon) {
    // Lets the anonymous visitor re-open their own result page later.
    response.cookies.set('golos_anon', anon, {
      httpOnly: true,
      sameSite: 'lax',
      secure: process.env.NODE_ENV === 'production',
      maxAge: 60 * 60 * 24 * 30,
    });
  }

  return response;
}
