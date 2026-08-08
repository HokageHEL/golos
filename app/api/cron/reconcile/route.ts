import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { applyCompletion } from '@/app/api/webhooks/assemblyai/route';

const STALE_AFTER_MINUTES = 30;
const GIVE_UP_AFTER_HOURS = 6;

export async function GET(request: Request) {
  if (request.headers.get('authorization') !== `Bearer ${process.env.CRON_SECRET}`) {
    return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
  }

  const db = serviceClient();
  const staleBefore = new Date(Date.now() - STALE_AFTER_MINUTES * 60_000).toISOString();
  const giveUpBefore = new Date(Date.now() - GIVE_UP_AFTER_HOURS * 3_600_000).toISOString();

  const { data: stuck } = await db
    .from('jobs')
    .select('id, provider_job_id, created_at')
    .eq('status', 'processing')
    .lt('created_at', staleBefore)
    .limit(50);

  let recovered = 0;
  let abandoned = 0;

  for (const job of stuck ?? []) {
    if (job.created_at < giveUpBefore) {
      await db
        .from('jobs')
        .update({ status: 'failed', error: 'timed_out', completed_at: new Date().toISOString() })
        .eq('id', job.id);
      abandoned += 1;
      continue;
    }

    if (job.provider_job_id) {
      await applyCompletion(job.provider_job_id);
      recovered += 1;
    }
  }

  return NextResponse.json({ checked: stuck?.length ?? 0, recovered, abandoned });
}
