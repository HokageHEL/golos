import { expect, test } from 'vitest';
import { FakeProvider } from '../fake';
import { WEBHOOK_SECRET_HEADER } from '../assemblyai';

const provider = new FakeProvider({ webhookSecret: 'secret' });

test('returns a finished two-speaker transcript', async () => {
  const result = await provider.fetchResult('fake-1');

  expect(result.status).toBe('done');
  expect(result.providerJobId).toBe('fake-1');
  expect(new Set(result.segments.map((s) => s.speaker))).toEqual(new Set(['A', 'B']));
  expect(result.text).toContain('Добрий день');
  expect(result.durationSeconds).toBeGreaterThan(0);
});

test('accepts a webhook carrying the matching secret', () => {
  const headers = new Headers({ [WEBHOOK_SECRET_HEADER]: 'secret' });
  expect(provider.verifyWebhook(headers)).toBe(true);
});

test('rejects a webhook with the wrong secret or no header at all', () => {
  expect(provider.verifyWebhook(new Headers({ [WEBHOOK_SECRET_HEADER]: 'nope' }))).toBe(false);
  expect(provider.verifyWebhook(new Headers())).toBe(false);
});

test('an empty configured secret does not accept an empty header', () => {
  // Buffer.from('') compares equal to itself, so an unset secret would
  // otherwise turn the header into a formality that anyone can satisfy.
  const misconfigured = new FakeProvider({ webhookSecret: '' });
  expect(misconfigured.verifyWebhook(new Headers({ [WEBHOOK_SECRET_HEADER]: '' }))).toBe(false);
});
