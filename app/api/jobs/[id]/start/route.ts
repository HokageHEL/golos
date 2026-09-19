import { cookies } from 'next/headers';
import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { transcriptionProvider } from '@/lib/transcription';
import { currentUser } from '@/lib/auth';
import { ANON_TRIAL_SECONDS, PAYG_RATE_CENTS } from '@/lib/billing/pricing';
import { MAX_AUDIO_SECONDS } from '@/lib/jobs/media';
import { UUID, apiError, appUrl, sameOrigin } from '@/lib/api';

export async function POST(request: Request, ctx: { params: Promise<{ id: string }> }) {
  if (!sameOrigin(request)) return apiError('forbidden', 403);
  const { id } = await ctx.params;
  if (!UUID.test(id)) return apiError('not_found', 404);
  try {
    const db = serviceClient();
    const { data: job, error } = await db.from('jobs')
      .select('id, storage_path, status, user_id, anon_key').eq('id', id).single();
    if (error && error.code !== 'PGRST116') throw error;
    if (!job) return apiError('not_found', 404);
    const user = await currentUser();
    const anon = (await cookies()).get('golos_anon')?.value;
    if (!(job.user_id ? job.user_id === user?.id : anon && job.anon_key === anon)) return apiError('not_found', 404);
    if (job.status !== 'awaiting_upload') return NextResponse.json({ ok: true });
    const { data: signed, error: signError } = await db.storage.from('audio').createSignedUrl(job.storage_path, 4 * 3600);
    if (signError || !signed) return apiError('upload_missing', 409);
    const provider = transcriptionProvider();
    const webhookUrl = `${appUrl()}/api/webhooks/assemblyai?job=${id}`;
    const { data: reservation, error: reserveError } = await db.rpc('reserve_job', {
      p_job_id: id, p_rate: PAYG_RATE_CENTS,
      p_max_seconds: job.user_id ? MAX_AUDIO_SECONDS : ANON_TRIAL_SECONDS,
    });
    if (reserveError) throw reserveError;
    if (reservation.error) return apiError(reservation.error, 409);
    if (reservation.already_started) return NextResponse.json({ ok: true });
    try {
      const { providerJobId } = await provider.submit({
        audioUrl: signed.signedUrl, webhookUrl, maxDurationMs: reservation.maxDurationSeconds * 1000,
      });
      const { error: updateError } = await db.from('jobs')
        .update({ provider_job_id: providerJobId, provider: provider.name }).eq('id', id);
      if (updateError) throw updateError;
    } catch (submitError) {
      // Never resubmit an ambiguous timeout: the provider may have accepted it.
      // Keep the claim for reconciliation instead of allowing duplicate charges.
      console.error('job submission failed', { jobId: id, error: submitError });
      return apiError('submission_failed', 502);
    }
    return NextResponse.json({ ok: true, maxDurationSeconds: reservation.maxDurationSeconds });
  } catch (error) {
    console.error('starting job failed', error);
    return apiError('unavailable', 503);
  }
}
