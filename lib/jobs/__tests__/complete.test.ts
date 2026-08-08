import { expect, test } from 'vitest';
import { buildCompletion } from '../complete';
import type { TranscriptionResult } from '@/lib/transcription/types';

const result: TranscriptionResult = {
  providerJobId: 'p1',
  status: 'done',
  segments: [
    { startMs: 0, endMs: 1000, speaker: 'A', text: 'Hello there.' },
    { startMs: 1000, endMs: 2000, speaker: 'B', text: 'Hi.' },
  ],
  text: 'Hello there. Hi.',
  durationSeconds: 600,
  language: 'en',
  error: null,
};

test('builds a transcript row with word count', () => {
  const plan = { minutesIncluded: 0, minutesUsed: 0 };
  const completion = buildCompletion({ jobId: 'j1', userId: 'u1', result, plan });

  expect(completion.transcript).toEqual({
    job_id: 'j1',
    text: 'Hello there. Hi.',
    segments: result.segments,
    word_count: 3,
  });
});

test('debits credit with a job-scoped idempotency key', () => {
  const plan = { minutesIncluded: 0, minutesUsed: 0 };
  const completion = buildCompletion({ jobId: 'j1', userId: 'u1', result, plan });

  expect(completion.ledgerEntry).toEqual({
    user_id: 'u1',
    delta_cents: -50, // 10 minutes at $0.05
    reason: 'transcription',
    job_id: 'j1',
    external_id: 'job:j1:debit',
  });
});

test('uses plan minutes instead of credit when available', () => {
  const plan = { minutesIncluded: 300, minutesUsed: 0 };
  const completion = buildCompletion({ jobId: 'j1', userId: 'u1', result, plan });

  expect(completion.ledgerEntry).toBeNull();
  expect(completion.minutesUsedDelta).toBe(10);
});

test('anonymous jobs produce no ledger entry', () => {
  const completion = buildCompletion({
    jobId: 'j1',
    userId: null,
    result,
    plan: { minutesIncluded: 0, minutesUsed: 0 },
  });

  expect(completion.ledgerEntry).toBeNull();
  expect(completion.anonSpendCents).toBe(50);
});

test('a failed result debits nothing and records the error', () => {
  const completion = buildCompletion({
    jobId: 'j1',
    userId: 'u1',
    result: { ...result, status: 'failed', error: 'Audio file is corrupt', segments: [] },
    plan: { minutesIncluded: 0, minutesUsed: 0 },
  });

  expect(completion.job.status).toBe('failed');
  expect(completion.job.error).toBe('Audio file is corrupt');
  expect(completion.ledgerEntry).toBeNull();
  expect(completion.transcript).toBeNull();
});
