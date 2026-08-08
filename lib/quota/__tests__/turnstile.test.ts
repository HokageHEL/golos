import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { verifyTurnstile } from '../../turnstile';

beforeEach(() => {
  process.env.TURNSTILE_SECRET_KEY = 'test-secret';
});

afterEach(() => {
  vi.unstubAllGlobals();
});

test('a null token returns false without calling fetch', async () => {
  const fetchMock = vi.fn();
  vi.stubGlobal('fetch', fetchMock);

  await expect(verifyTurnstile(null, '1.2.3.4')).resolves.toBe(false);
  expect(fetchMock).not.toHaveBeenCalled();
});

test('a rejected fetch returns false and does not throw', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => {
      throw new Error('ECONNREFUSED');
    }),
  );

  await expect(verifyTurnstile('token', '1.2.3.4')).resolves.toBe(false);
});

test('a non-2xx response returns false', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify({ success: true }), { status: 500 })),
  );

  await expect(verifyTurnstile('token', '1.2.3.4')).resolves.toBe(false);
});

test('{ success: true } returns true', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify({ success: true }), { status: 200 })),
  );

  await expect(verifyTurnstile('token', '1.2.3.4')).resolves.toBe(true);
});

test('{ success: false } returns false', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify({ success: false }), { status: 200 })),
  );

  await expect(verifyTurnstile('token', '1.2.3.4')).resolves.toBe(false);
});
