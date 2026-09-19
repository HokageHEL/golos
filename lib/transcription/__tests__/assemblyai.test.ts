import { afterEach, expect, test, vi } from 'vitest';
import { AssemblyAIProvider } from '../assemblyai';
import completed from '../__fixtures__/completed.json';

const provider = new AssemblyAIProvider({ apiKey: 'k', webhookSecret: 's' });

afterEach(() => vi.restoreAllMocks());

test('maps utterances to segments with ms timings', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify(completed), { status: 200 })),
  );

  const result = await provider.fetchResult('abc123');

  expect(result!.status).toBe('done');
  expect(result!.durationSeconds).toBe(1122);
  expect(result!.language).toBe('en');
  expect(result!.segments).toEqual([
    { startMs: 0, endMs: 1500, speaker: 'A', text: 'Hello there.' },
    { startMs: 1500, endMs: 3000, speaker: 'B', text: 'Hi.' },
  ]);
});

test('reports provider-side errors as failed', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(
      async () =>
        new Response(
          JSON.stringify({ id: 'x', status: 'error', error: 'Audio file is corrupt' }),
          { status: 200 },
        ),
    ),
  );

  const result = await provider.fetchResult('x');
  expect(result!.status).toBe('failed');
  expect(result!.error).toBe('Audio file is corrupt');
});

test('sends audio_end_at only when a cap is given', async () => {
  const fetchMock = vi.fn<(input: RequestInfo | URL, init: RequestInit) => Promise<Response>>(
    async () =>
      new Response(JSON.stringify({ id: 'new' }), { status: 200 }),
  );
  vi.stubGlobal('fetch', fetchMock);

  await provider.submit({ audioUrl: 'https://x/a.mp3', webhookUrl: 'https://y/w' });
  expect(JSON.parse(fetchMock.mock.calls[0][1].body as string)).not.toHaveProperty('audio_end_at');

  await provider.submit({
    audioUrl: 'https://x/a.mp3',
    webhookUrl: 'https://y/w',
    maxDurationMs: 600_000,
  });
  expect(JSON.parse(fetchMock.mock.calls[1][1].body as string).audio_end_at).toBe(600_000);
});

test('verifyWebhook accepts the right secret and rejects anything else', () => {
  expect(provider.verifyWebhook(new Headers({ 'x-golos-webhook-secret': 's' }))).toBe(true);
  expect(provider.verifyWebhook(new Headers({ 'x-golos-webhook-secret': 'nope' }))).toBe(false);
  expect(provider.verifyWebhook(new Headers())).toBe(false);
});

test.each(['queued', 'processing'])('does not complete a %s provider job', async (status) => {
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ id: 'x', status })));
  expect(await provider.fetchResult('x')).toBeNull();
});
test('preserves text when provider returns no utterances', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => Response.json({ id: 'x', status: 'completed', text: 'Quiet speaker', audio_duration: 2.5 })));
  const result = await provider.fetchResult('x');
  expect(result?.segments).toEqual([{ startMs: 0, endMs: 2500, speaker: null, text: 'Quiet speaker' }]);
  expect(result?.durationSeconds).toBe(3);
});
test('rejects an unset secret instead of crashing or accepting an empty header', () => {
  const unconfigured = new AssemblyAIProvider({ apiKey: 'x', webhookSecret: undefined as unknown as string });
  expect(unconfigured.verifyWebhook(new Headers({ 'x-golos-webhook-secret': '' }))).toBe(false);
});
