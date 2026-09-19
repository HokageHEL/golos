import { expect, test, vi } from 'vitest';
vi.mock('@/lib/auth', () => ({ currentUser: vi.fn() }));
vi.mock('@/lib/supabase/server', () => ({ serviceClient: vi.fn() }));
import { POST } from '../route';
import { currentUser } from '@/lib/auth';
const valid = { filename: 'talk.mp3', mimeType: 'audio/mpeg', fingerprint: 'fp', size: 50, turnstileToken: null };
const req = (body: unknown) => new Request('http://localhost/api/jobs/init', { method: 'POST', body: JSON.stringify(body) });
test.each([null, {}, { ...valid, filename: '../\nheader' }, { ...valid, size: -1 }, { ...valid, fingerprint: {} }])('rejects malformed requests before accessing services', async (body) => {
  expect((await POST(req(body))).status).toBe(400);
  expect(currentUser).not.toHaveBeenCalled();
});
test('rejects oversized files before quota or storage writes', async () => {
  expect((await POST(req({ ...valid, size: 100 * 1024 * 1024 }))).status).toBe(413);
});
test('rejects cross-origin state changes', async () => {
  const request = req(valid); request.headers.set('origin', 'https://attacker.invalid');
  expect((await POST(request)).status).toBe(403);
});
