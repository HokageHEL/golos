import { beforeEach, expect, test, vi } from 'vitest';
const mocks = vi.hoisted(() => ({ fetchResult: vi.fn(), rpc: vi.fn() }));
vi.mock('@/lib/supabase/server', () => ({ serviceClient: () => ({ rpc: mocks.rpc }) }));
vi.mock('@/lib/transcription', () => ({ transcriptionProvider: () => ({ fetchResult: mocks.fetchResult }) }));
import { applyCompletion } from '../apply-completion';
beforeEach(() => vi.resetAllMocks());
test('pending provider work never writes or charges', async () => {
  mocks.fetchResult.mockResolvedValue(null);
  expect(await applyCompletion('pending')).toBe(false);
  expect(mocks.rpc).not.toHaveBeenCalled();
});
test('completion uses one atomic transaction and respects replay results', async () => {
  mocks.fetchResult.mockResolvedValue({ status: 'done', text: 'Two words', segments: [], durationSeconds: 12 });
  mocks.rpc.mockResolvedValue({ data: true, error: null });
  expect(await applyCompletion('id')).toBe(true);
  expect(mocks.rpc).toHaveBeenCalledWith('complete_job', expect.objectContaining({ p_provider_id: 'id', p_result: expect.objectContaining({ wordCount: 2 }) }));
  mocks.rpc.mockResolvedValue({ data: false, error: null });
  expect(await applyCompletion('id')).toBe(false);
});
test('database failures propagate for webhook retries', async () => {
  mocks.fetchResult.mockResolvedValue({ status: 'done', text: '' });
  mocks.rpc.mockResolvedValue({ error: new Error('database unavailable') });
  await expect(applyCompletion('id')).rejects.toThrow('database unavailable');
});
