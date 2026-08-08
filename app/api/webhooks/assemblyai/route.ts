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

  if (completion.transcript) {
    const { error: transcriptError } = await db
      .from('transcripts')
      .upsert(completion.transcript, { onConflict: 'job_id' });
    if (transcriptError) throw transcriptError;
  }

  if (completion.ledgerEntry) {
    // Duplicate delivery collides on external_id; ignore that one error only.
    const { error } = await db.from('ledger').insert(completion.ledgerEntry);
    if (error && error.code !== '23505') throw error;
  }

  if (completion.minutesUsedDelta > 0 && job.user_id) {
    const { error: minutesError } = await db.rpc('add_minutes_used', {
      p_user_id: job.user_id,
      p_minutes: completion.minutesUsedDelta,
    });
    if (minutesError) throw minutesError;
  }

  if (completion.anonSpendCents > 0) {
    await recordAnonSpend(redisStore(), completion.anonSpendCents);
  }

  const { error: jobUpdateError } = await db
    .from('jobs')
    .update(completion.job)
    .eq('id', job.id);
  if (jobUpdateError) throw jobUpdateError;
}
