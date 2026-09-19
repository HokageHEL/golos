import { randomBytes, randomUUID } from 'node:crypto';
import { cookies } from 'next/headers';
import { apiError, sameOrigin } from '@/lib/api';
import { MAX_UPLOAD_BYTES } from '@/lib/jobs/media';
import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { isSupportedAudio, storagePathFor } from '@/lib/jobs/media';
import { anonKey } from '@/lib/quota/identity';
import { claimAnonTrial } from '@/lib/quota/anon';
import { redisStore } from '@/lib/quota/redis';
import { verifyTurnstile } from '@/lib/turnstile';
import { currentUser } from '@/lib/auth';
import { ANON_TRIAL_SECONDS, PAYG_RATE_CENTS } from '@/lib/billing/pricing';

export async function POST(request: Request) {
  if (!sameOrigin(request)) return apiError('forbidden', 403);
  const body = await request.json().catch(() => null);
  if (!body || typeof body.filename !== 'string' || !body.filename.trim() ||
      body.filename.length > 240 || /[\x00-\x1f]/.test(body.filename) ||
      typeof body.mimeType !== 'string' || !Number.isSafeInteger(body.size) || body.size <= 0 ||
      typeof body.fingerprint !== 'string' || body.fingerprint.length > 200 ||
      (body.turnstileToken != null && typeof body.turnstileToken !== 'string')) {
    return apiError('invalid_request', 400);
  }
  if (body.size > MAX_UPLOAD_BYTES) return apiError('too_large', 413);
  try {
    if (!isSupportedAudio(body.mimeType)) {
      return NextResponse.json({ error: 'unsupported_format' }, { status: 415 });
    }

    const ip = request.headers.get('x-forwarded-for')?.split(',')[0].trim() ?? '0.0.0.0';
    const user = await currentUser();
    const db = serviceClient();

    let maxDurationSeconds: number | null = null;
    let anon: string | null = null;

    if (user) {
      const { data: account, error: accountError } = await db
        .from('accounts')
        .select('balance_cents, minutes_included, minutes_used, period_end, subscription_status')
        .eq('user_id', user.id)
        .single();

      if (accountError) throw accountError;
      const hasPlanMinutes = account?.subscription_status === 'active' && Date.parse(account.period_end) > Date.now() && (account.minutes_included > account.minutes_used);
      if (!hasPlanMinutes && (account?.balance_cents ?? 0) < PAYG_RATE_CENTS) {
        return NextResponse.json({ error: 'insufficient_balance' }, { status: 402 });
      }
    } else {
      if (!body.fingerprint) return apiError('invalid_request', 400);
      const quotaKey = anonKey(ip, body.fingerprint);
      const existing = (await cookies()).get('golos_anon')?.value;
      anon = existing && /^[a-f0-9]{64}$/.test(existing) ? existing : randomBytes(32).toString('hex');

      // Redis holds the only record of who has already used their free trial.
      // If it is unreachable we cannot tell a first-time visitor from a repeat
      // one, so we refuse rather than hand out unlimited free transcriptions.
      // Same fail-closed reasoning as verifyTurnstile, and for the same reason:
      // an unhandled rejection here turns an outage into a 500.
      let claim;
      try {
        claim = await claimAnonTrial({
          key: quotaKey,
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

    const jobId = randomUUID();
    const path = storagePathFor(jobId, body.filename);
    const { data: job, error } = await db
      .from('jobs')
      .insert({
        id: jobId,
        user_id: user?.id ?? null,
        anon_key: anon,
        filename: body.filename,
        storage_path: path,
        status: 'awaiting_upload',
      })
      .select('id')
      .single();

    if (error || !job) {
      return NextResponse.json({ error: 'generic' }, { status: 500 });
    }

    const { data: signed, error: signError } = await db.storage
      .from('audio')
      .createSignedUploadUrl(path);

    if (signError || !signed) {
      await db.from('jobs').delete().eq('id', job.id);
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
        path: '/',
        httpOnly: true,
        sameSite: 'lax',
        secure: process.env.NODE_ENV === 'production',
        maxAge: 60 * 60 * 24 * 30,
      });
    }

    return response;
  } catch (error) {
    console.error('job initialization failed', error);
    return apiError('unavailable', 503);
  }
}
