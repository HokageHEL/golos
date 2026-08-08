import { expect, test } from 'vitest';
import { toTurns } from '../turns';
import type { Segment } from '../types';

const seg = (startMs: number, endMs: number, speaker: string | null, text: string): Segment =>
  ({ startMs, endMs, speaker, text });

test('merges consecutive segments from the same speaker', () => {
  const turns = toTurns([
    seg(0, 1000, 'A', 'Hello there.'),
    seg(1000, 2000, 'A', 'How are you?'),
  ]);

  expect(turns).toEqual([
    { startMs: 0, endMs: 2000, speaker: 'A', text: 'Hello there. How are you?' },
  ]);
});

test('starts a new turn when the speaker changes', () => {
  const turns = toTurns([
    seg(0, 1000, 'A', 'Hello.'),
    seg(1000, 2000, 'B', 'Hi.'),
    seg(2000, 3000, 'A', 'Bye.'),
  ]);

  expect(turns.map((t) => t.speaker)).toEqual(['A', 'B', 'A']);
  expect(turns[1]).toEqual({ startMs: 1000, endMs: 2000, speaker: 'B', text: 'Hi.' });
});

test('handles a transcript with no diarization', () => {
  const turns = toTurns([seg(0, 1000, null, 'One.'), seg(1000, 2000, null, 'Two.')]);
  expect(turns).toEqual([{ startMs: 0, endMs: 2000, speaker: null, text: 'One. Two.' }]);
});

test('empty input yields no turns', () => {
  expect(toTurns([])).toEqual([]);
});
