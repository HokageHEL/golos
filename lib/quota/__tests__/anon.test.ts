import { beforeEach, expect, test, vi } from 'vitest';
import { claimAnonTrial } from '../anon';
import type { QuotaStore } from '../redis';
import { ANON_DAILY_BUDGET_CENTS } from '@/lib/billing/pricing';

function fakeStore(overrides: Partial<QuotaStore> = {}): QuotaStore {
  const claimed = new Set<string>();
  return {
    claimOnce: vi.fn(async (key: string) => {
      if (claimed.has(key)) return false;
      claimed.add(key);
      return true;
    }),
    incrementBy: vi.fn(async () => 0),
    get: vi.fn(async () => 0),
    ...overrides,
  };
}

const ok = () => Promise.resolve(true);

beforeEach(() => vi.clearAllMocks());

test('grants the first claim', async () => {
  const result = await claimAnonTrial({ key: 'k1', store: fakeStore(), verifyBot: ok });
  expect(result).toEqual({ granted: true, maxDurationSeconds: 600 });
});

test('refuses a second claim by the same visitor', async () => {
  const store = fakeStore();
  await claimAnonTrial({ key: 'k1', store, verifyBot: ok });
  const second = await claimAnonTrial({ key: 'k1', store, verifyBot: ok });
  expect(second).toEqual({ granted: false, reason: 'already_used' });
});

test('refuses everyone once the daily budget is gone', async () => {
  const store = fakeStore({ get: vi.fn(async () => ANON_DAILY_BUDGET_CENTS) });
  const result = await claimAnonTrial({ key: 'fresh-key', store, verifyBot: ok });
  expect(result).toEqual({ granted: false, reason: 'budget_exhausted' });
});

test('refuses when bot verification fails, without consuming the claim', async () => {
  const store = fakeStore();
  const result = await claimAnonTrial({
    key: 'k1',
    store,
    verifyBot: () => Promise.resolve(false),
  });

  expect(result).toEqual({ granted: false, reason: 'bot_check_failed' });
  expect(store.claimOnce).not.toHaveBeenCalled();
});
