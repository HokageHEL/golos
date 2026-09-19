import type { Segment } from './types';

export function validateEdits(value: unknown): Segment[] | null {
  if (!Array.isArray(value) || value.length > 20000) return null;
  let length = 0;
  let previousEnd = 0;
  for (const s of value) {
    if (!s || !Number.isSafeInteger(s.startMs) || !Number.isSafeInteger(s.endMs) ||
      s.startMs < previousEnd || s.endMs < s.startMs || s.endMs > 7_200_000 ||
      typeof s.text !== 'string' || (s.speaker !== null && (typeof s.speaker !== 'string' || s.speaker.length > 100))) return null;
    length += s.text.length;
    if (length > 1_000_000) return null;
    previousEnd = s.startMs;
  }
  return value.map(({ startMs, endMs, text, speaker }) => ({ startMs, endMs, text, speaker }));
}
