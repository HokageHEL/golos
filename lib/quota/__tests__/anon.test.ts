import { expect, test, vi } from 'vitest';
import { claimAnonTrial, TRIAL_RESERVATION_CENTS } from '../anon';
import type { QuotaStore } from '../redis';
import { ANON_DAILY_BUDGET_CENTS } from '@/lib/billing/pricing';

function store(): QuotaStore {
  const claimed = new Set<string>();
  let spent = 0;
  return { reserveTrial: vi.fn(async (key, _budget, _ttl, cost, ceiling) => {
    if (claimed.has(key)) return 'already_used';
    if (spent + cost > ceiling) return 'budget_exhausted';
    claimed.add(key); spent += cost; return 'granted';
  }) };
}
const verifyBot = async () => true;
test('grants one claim and refuses repeats', async () => {
  const db = store();
  expect(await claimAnonTrial({ key: 'a', store: db, verifyBot })).toEqual({ granted: true, maxDurationSeconds: 600 });
  expect(await claimAnonTrial({ key: 'a', store: db, verifyBot })).toEqual({ granted: false, reason: 'already_used' });
});
test('reserves the entire trial cost before concurrent jobs can exceed the ceiling', async () => {
  const db = store();
  const claims = await Promise.all(Array.from({ length: 250 }, (_, i) => claimAnonTrial({ key: String(i), store: db, verifyBot })));
  expect(claims.filter((c) => c.granted)).toHaveLength(Math.floor(ANON_DAILY_BUDGET_CENTS / TRIAL_RESERVATION_CENTS));
});
test('bot rejection never consumes quota', async () => {
  const db = store();
  expect(await claimAnonTrial({ key: 'a', store: db, verifyBot: async () => false })).toEqual({ granted: false, reason: 'bot_check_failed' });
  expect(db.reserveTrial).not.toHaveBeenCalled();
});
