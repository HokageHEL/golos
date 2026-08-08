import { expect, test } from 'vitest';
import { toPlainText } from '../txt';

test('omits labels entirely when there is only one speaker', () => {
  const txt = toPlainText(
    [
      { startMs: 0, endMs: 1000, speaker: 'A', text: 'One.' },
      { startMs: 1000, endMs: 2000, speaker: 'A', text: 'Two.' },
    ],
    (n) => `Speaker ${n}`,
  );
  expect(txt).toBe('One.\n\nTwo.');
});

test('labels every turn when speakers differ', () => {
  const txt = toPlainText(
    [
      { startMs: 0, endMs: 1000, speaker: 'A', text: 'One.' },
      { startMs: 1000, endMs: 2000, speaker: 'B', text: 'Two.' },
    ],
    (n) => `Speaker ${n}`,
  );
  expect(txt).toBe('Speaker 1: One.\n\nSpeaker 2: Two.');
});
