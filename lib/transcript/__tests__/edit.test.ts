import { expect, test } from 'vitest';
import { validateEdits } from '../edit';
const segment = { startMs: 0, endMs: 2000, text: 'Edited text', speaker: 'A' };
test('accepts edits and discards unknown fields', () => {
  expect(validateEdits([{ ...segment, user_id: 'other' }])).toEqual([segment]);
});
test.each([null, {}, [{ ...segment, startMs: -1 }], [{ ...segment, endMs: NaN }], [{ ...segment, text: null }], [{ ...segment, text: 'a'.repeat(1000001) }]])('rejects malformed transcript edits', (input) => {
  expect(validateEdits(input)).toBeNull();
});
