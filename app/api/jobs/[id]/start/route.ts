import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { transcriptionProvider } from '@/lib/transcription';
import { ANON_TRIAL_SECONDS } from '@/lib/billing/pricing';

const SIGNED_READ_TTL_SECONDS = 60 * 60 * 4;

export async function POST(_: Request, ctx: { params: Promise<{ id: string }> }) {
  const { id } = await ctx.params;
  const db = serviceClient();

  const { data: job } = await db
    .from('jobs')
    .select('id, storage_path, status, user_id')
    .eq('id', id)
    .single();

  if (!job) return NextResponse.json({ error: 'not_found' }, { status: 404 });
  if (job.status !== 'awaiting_upload') {
    return NextResponse.json({ ok: true }); // already started; nothing to do
  }

  const { data: signed } = await db.storage
    .from('audio')
    .createSignedUrl(job.storage_path, SIGNED_READ_TTL_SECONDS);

  if (!signed) return NextResponse.json({ error: 'generic' }, { status: 500 });

  const isAnonymous = job.user_id === null;

  const { providerJobId } = await transcriptionProvider().submit({
    audioUrl: signed.signedUrl,
    webhookUrl: `${process.env.NEXT_PUBLIC_APP_URL}/api/webhooks/assemblyai`,
    // Bounds the cost of a free trial no matter how long the file actually is.
    maxDurationMs: isAnonymous ? ANON_TRIAL_SECONDS * 1000 : undefined,
  });

  await db
    .from('jobs')
    .update({ provider_job_id: providerJobId, status: 'processing', truncated: isAnonymous })
    .eq('id', id);

  return NextResponse.json({ ok: true });
}
