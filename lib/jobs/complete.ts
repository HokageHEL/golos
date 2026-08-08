import { chargeFor, type PlanState } from '@/lib/billing/cost';
import { wordCount } from '@/lib/transcript/turns';
import type { TranscriptionResult } from '@/lib/transcription/types';
import type { Segment } from '@/lib/transcript/types';

export type Completion = {
  job: {
    status: 'done' | 'failed';
    duration_seconds: number;
    language: string | null;
    cost_cents: number;
    error: string | null;
    completed_at: string;
  };
  transcript: {
    job_id: string;
    text: string;
    segments: Segment[];
    word_count: number;
  } | null;
  ledgerEntry: {
    user_id: string;
    delta_cents: number;
    reason: string;
    job_id: string;
    external_id: string;
  } | null;
  minutesUsedDelta: number;
  anonSpendCents: number;
};

/**
 * Pure decision function: given a provider result, decide what to write.
 * Kept separate from the database calls so the money rules are testable.
 */
export function buildCompletion(input: {
  jobId: string;
  userId: string | null;
  result: TranscriptionResult;
  plan: PlanState;
}): Completion {
  const { jobId, userId, result, plan } = input;
  const completedAt = new Date().toISOString();

  if (result.status === 'failed') {
    return {
      job: {
        status: 'failed',
        duration_seconds: 0,
        language: null,
        cost_cents: 0,
        error: result.error,
        completed_at: completedAt,
      },
      transcript: null,
      ledgerEntry: null,
      minutesUsedDelta: 0,
      anonSpendCents: 0,
    };
  }

  const charge = chargeFor(result.durationSeconds, plan);

  return {
    job: {
      status: 'done',
      duration_seconds: result.durationSeconds,
      language: result.language,
      cost_cents: charge.centsFromCredit,
      error: null,
      completed_at: completedAt,
    },
    transcript: {
      job_id: jobId,
      text: result.text,
      segments: result.segments,
      word_count: wordCount(result.text),
    },
    // Emitted for every signed-in job — even a plan-covered one with
    // delta_cents: 0 — because this row, not a second mechanism, is the
    // single exactly-once claim on consumption. A replayed webhook collides
    // on external_id whether or not any credit was charged.
    ledgerEntry:
      userId !== null
        ? {
            user_id: userId,
            // 0 - x, not -x: unary negation of 0 is -0, which is a
            // different value than 0 to strict equality checks.
            delta_cents: 0 - charge.centsFromCredit,
            reason: 'transcription',
            job_id: jobId,
            external_id: `job:${jobId}:debit`,
          }
        : null,
    minutesUsedDelta: charge.minutesFromPlan,
    anonSpendCents: userId === null ? charge.centsFromCredit : 0,
  };
}
