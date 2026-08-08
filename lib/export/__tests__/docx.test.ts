import { expect, test } from 'vitest';
import { toDocx } from '../docx';

test('produces a zip package with document.xml', async () => {
  const buf = await toDocx(
    [{ startMs: 0, endMs: 1000, speaker: 'A', text: 'Hello.' }],
    (n) => `Speaker ${n}`,
  );

  expect(buf.length).toBeGreaterThan(0);
  // A .docx is a zip; every zip starts with the local file header magic "PK\x03\x04".
  expect(buf.subarray(0, 4)).toEqual(Buffer.from([0x50, 0x4b, 0x03, 0x04]));
  expect(buf.includes(Buffer.from('word/document.xml'))).toBe(true);
});
