import { after } from 'next/server';
import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { transcriptionProvider } from '@/lib/transcription';
import { buildCompletion } from '@/lib/jobs/complete';
import { recordAnonSpend } from '@/lib/quota/anon';
import { redisStore } from '@/lib/quota/redis';

export async function POST(request: Request) {
  const provider = transcriptionProvider();

  if (!provider.verifyWebhook(request.headers)) {
    return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
  }

  const body = (await request.json()) as { transcript_id: string; status: string };

  // Respond immediately — the provider gives us 10 seconds before it retries,
  // and fetching plus storing a two-hour transcript can exceed that.
  after(async () => {
    await applyCompletion(body.transcript_id);
  });

  return NextResponse.json({ ok: true });
}

export async function applyCompletion(providerJobId: string): Promise<void> {
  const db = serviceClient();

  const { data: job } = await db
    .from('jobs')
    .select('id, user_id, status')
    .eq('provider_job_id', providerJobId)
    .single();

  if (!job) return;
  if (job.status === 'done' || job.status === 'failed') return; // already applied

  const result = await transcriptionProvider().fetchResult(providerJobId);

  const { data: account } = job.user_id
    ? await db
        .from('accounts')
        .select('minutes_included, minutes_used')
        .eq('user_id', job.user_id)
        .single()
    : { data: null };

  const completion = buildCompletion({
    jobId: job.id,
    userId: job.user_id,
    result,
    plan: {
      minutesIncluded: account?.minutes_included ?? 0,
      minutesUsed: account?.minutes_used ?? 0,
    },
  });

  // 1. Transcript upsert first — idempotent on job_id, safe to repeat.
  // Doing this before the claim below means a retry after a mid-way
  // failure can still make progress instead of colliding forever.
  if (completion.transcript) {
    const { error: transcriptError } = await db
      .from('transcripts')
      .upsert(completion.transcript, { onConflict: 'job_id' });
    if (transcriptError) throw transcriptError;
  }

  // 2. The ledger insert IS the exactly-once claim on consumption (even a
  // plan-covered job with delta_cents: 0 gets a row — see buildCompletion).
  // 3. Only run consumption side-effects if this delivery is the one that
  // actually won the claim; a 23505 collision means a prior delivery
  // already applied them, so skip to avoid double-consuming plan minutes.
  let claimed = true;
  if (completion.ledgerEntry) {
    const { error } = await db.from('ledger').insert(completion.ledgerEntry);
    if (error) {
      if (error.code !== '23505') throw error;
      claimed = false;
    }
  }

  if (claimed) {
    if (completion.minutesUsedDelta > 0 && job.user_id) {
      const { error: minutesError } = await db.rpc('add_minutes_used', {
        p_user_id: job.user_id,
        p_minutes: completion.minutesUsedDelta,
      });
      if (minutesError) throw minutesError;
    }

    // Anonymous jobs have no user_id, so they can never insert a ledger row
    // to claim against — recordAnonSpend may double-count on a retry. That
    // only tightens the daily free-trial budget ceiling; it fails safe
    // (fewer free trials granted), never overcharges anyone, so it is left
    // as-is rather than building a second claim mechanism for it.
    if (completion.anonSpendCents > 0) {
      await recordAnonSpend(redisStore(), completion.anonSpendCents);
    }
  }

  // 4. Status update runs either way so a retry after a collision can still
  // finish the job instead of getting stuck in 'processing' forever.
  const { error: jobUpdateError } = await db
    .from('jobs')
    .update(completion.job)
    .eq('id', job.id);
  if (jobUpdateError) throw jobUpdateError;
}
