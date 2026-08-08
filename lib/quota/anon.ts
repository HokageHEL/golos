import { ANON_DAILY_BUDGET_CENTS, ANON_TRIAL_SECONDS } from '@/lib/billing/pricing';
import type { QuotaStore } from './redis';

const TRIAL_TTL_SECONDS = 60 * 60 * 24 * 30;

export type ClaimResult =
  | { granted: true; maxDurationSeconds: number }
  | { granted: false; reason: 'already_used' | 'budget_exhausted' | 'bot_check_failed' };

export function budgetKey(now = new Date()): string {
  return `anon:spend:${now.toISOString().slice(0, 10)}`;
}

export async function claimAnonTrial(input: {
  key: string;
  store: QuotaStore;
  verifyBot: () => Promise<boolean>;
  now?: Date;
}): Promise<ClaimResult> {
  if (!(await input.verifyBot())) {
    return { granted: false, reason: 'bot_check_failed' };
  }

  const spent = await input.store.get(budgetKey(input.now));
  if (spent >= ANON_DAILY_BUDGET_CENTS) {
    return { granted: false, reason: 'budget_exhausted' };
  }

  const claimed = await input.store.claimOnce(`anon:trial:${input.key}`, TRIAL_TTL_SECONDS);
  if (!claimed) return { granted: false, reason: 'already_used' };

  return { granted: true, maxDurationSeconds: ANON_TRIAL_SECONDS };
}

export async function recordAnonSpend(
  store: QuotaStore,
  cents: number,
  now = new Date(),
): Promise<void> {
  await store.incrementBy(budgetKey(now), cents, 60 * 60 * 48);
}
