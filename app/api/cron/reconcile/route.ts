import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { applyCompletion } from '@/lib/jobs/apply-completion';

const STALE_AFTER_MINUTES = 30;
const GIVE_UP_AFTER_HOURS = 6;
/**
 * Close the tab mid-upload and `POST /api/jobs/[id]/start` never fires, so the
 * job sits at `awaiting_upload` forever — invisible to the `processing` sweep
 * below, with its bytes possibly already paid for in storage. Two hours is far
 * longer than the slowest plausible 120 MB upload.
 */
const ABANDON_UPLOAD_AFTER_HOURS = 2;

export async function GET(request: Request) {
  const secret = process.env.CRON_SECRET;
  if (!secret) {
    // An unset or empty secret must lock the endpoint, not turn
    // "Bearer undefined" / "Bearer " into a valid credential.
    return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
  }
  if (request.headers.get('authorization') !== `Bearer ${secret}`) {
    return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
  }

  const db = serviceClient();
  const staleBefore = new Date(Date.now() - STALE_AFTER_MINUTES * 60_000).toISOString();
  const giveUpBefore = new Date(Date.now() - GIVE_UP_AFTER_HOURS * 3_600_000).toISOString();

  const { data: stuck, error: readError } = await db
    .from('jobs')
    .select('id, provider_job_id, created_at')
    .eq('status', 'processing')
    .lt('created_at', staleBefore)
    .order('created_at', { ascending: true })
    .limit(50);

  if (readError) return NextResponse.json({ error: 'unavailable' }, { status: 503 });
  let recovered = 0;
  let abandoned = 0;
  let errors = 0;

  for (const job of stuck ?? []) {
    try {
      if (job.provider_job_id && await applyCompletion(job.provider_job_id)) {
        recovered += 1;
        continue;
      }
      if (job.created_at < giveUpBefore && !job.provider_job_id) {
        const { error } = await db
          .from('jobs')
          .update({ status: 'failed', error: 'timed_out', completed_at: new Date().toISOString() })
          .eq('id', job.id).eq('status', 'processing');
        if (error) throw error;
        abandoned += 1;
        continue;
      }
    } catch (err) {
      console.error(`reconcile: job ${job.id} failed`, err);
      errors += 1;
    }
  }

  const reaped = await reapAbandonedUploads(db);

  return NextResponse.json({ checked: stuck?.length ?? 0, recovered, abandoned, reaped, errors });
}

/**
 * Fail jobs whose upload never completed, and drop whatever bytes did land.
 * Nothing was ever sent to the provider, so no money is involved — this is
 * about not leaving dead rows and unreferenced storage objects behind.
 */
async function reapAbandonedUploads(db: ReturnType<typeof serviceClient>): Promise<number> {
  const cutoff = new Date(Date.now() - ABANDON_UPLOAD_AFTER_HOURS * 3_600_000).toISOString();

  const { data: orphans, error } = await db
    .from('jobs')
    .select('id, storage_path')
    .eq('status', 'awaiting_upload')
    .lt('created_at', cutoff)
    .order('created_at', { ascending: true })
    .limit(50);

  if (error) {
    console.error('reconcile: reading abandoned uploads failed', error);
    return 0;
  }

  let reaped = 0;

  for (const job of orphans ?? []) {
    try {
      const { data: claimed, error: updateError } = await db
        .from('jobs')
        .update({
          status: 'failed',
          error: 'abandoned_upload',
          completed_at: new Date().toISOString(),
        })
        .eq('id', job.id).eq('status', 'awaiting_upload').select('id').maybeSingle();
      if (updateError) throw updateError;
      if (!claimed) continue;
      // Claim before removing media, so a simultaneous start cannot lose its file.
      if (job.storage_path) {
        const { error: removeError } = await db.storage.from('audio').remove([job.storage_path]);
        if (removeError) console.warn('Abandoned media cleanup failed', removeError);
      }

      reaped += 1;
    } catch (err) {
      console.error(`reconcile: reaping job ${job.id} failed`, err);
    }
  }

  return reaped;
}
