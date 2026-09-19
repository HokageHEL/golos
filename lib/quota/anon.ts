import { ANON_DAILY_BUDGET_CENTS, ANON_TRIAL_SECONDS } from '@/lib/billing/pricing';
import type { QuotaStore } from './redis';

// Conservative per-trial reservation, including abandoned uploads. The ceiling
// is reserved before any provider work, so concurrent jobs cannot overspend it.
export const TRIAL_RESERVATION_CENTS = 3;
const TRIAL_TTL_SECONDS = 60 * 60 * 24 * 30;
export type ClaimResult =
  | { granted: true; maxDurationSeconds: number }
  | { granted: false; reason: 'already_used' | 'budget_exhausted' | 'bot_check_failed' };
export function budgetKey(now = new Date()): string {
  return `anon:spend:${now.toISOString().slice(0, 10)}`;
}
export async function claimAnonTrial(input: {
  key: string; store: QuotaStore; verifyBot: () => Promise<boolean>; now?: Date;
}): Promise<ClaimResult> {
  if (!(await input.verifyBot())) return { granted: false, reason: 'bot_check_failed' };
  const result = await input.store.reserveTrial(
    `anon:trial:${input.key}`, budgetKey(input.now), TRIAL_TTL_SECONDS,
    TRIAL_RESERVATION_CENTS, ANON_DAILY_BUDGET_CENTS,
  );
  return result === 'granted'
    ? { granted: true, maxDurationSeconds: ANON_TRIAL_SECONDS }
    : { granted: false, reason: result };
}
