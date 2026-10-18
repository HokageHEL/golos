import { expect, test } from 'vitest';
import type { Segment } from '../../../lib/transcript/types';
import { findTranscriptMatches, formatTime, segmentAtTime } from '../review';

const segments: Segment[] = [
  { startMs: 500, endMs: 2000, speaker: null, text: 'Привіт, Київ! Привіт.' },
  { startMs: 3000, endMs: 5000, speaker: null, text: 'Check C++ and [notes].' },
];

test('playback highlights segment boundaries but leaves silence unhighlighted', () => {
  expect(segmentAtTime(segments, 0)).toBe(-1);
  expect(segmentAtTime(segments, 0.5)).toBe(0);
  expect(segmentAtTime(segments, 1.999)).toBe(0);
  expect(segmentAtTime(segments, 2)).toBe(-1);
  expect(segmentAtTime(segments, 3)).toBe(1);
  expect(segmentAtTime(segments, 5)).toBe(-1);
  expect(segmentAtTime([], 1)).toBe(-1);
});

test('search finds every Ukrainian occurrence with offsets usable by textarea selection', () => {
  const matches = findTranscriptMatches(segments, 'ПРИВІТ');
  expect(matches).toHaveLength(2);
  for (const match of matches) {
    expect(segments[match.segmentIndex].text.slice(match.start, match.end)).toBe('Привіт');
  }
  const emoji = [{ ...segments[0], text: '🎙 Київ київ' }];
  expect(findTranscriptMatches(emoji, 'КИЇВ').map(match => [match.start, match.end])).toEqual([[3, 7], [8, 12]]);
});

test('search treats regex characters literally and uses current edited text', () => {
  expect(findTranscriptMatches(segments, 'C++')).toEqual([{ segmentIndex: 1, start: 6, end: 9 }]);
  expect(findTranscriptMatches(segments, '[notes]')).toEqual([{ segmentIndex: 1, start: 14, end: 21 }]);
  expect(findTranscriptMatches([{ ...segments[0], text: 'Edited phrase' }], 'Привіт')).toEqual([]);
  expect(findTranscriptMatches(segments, '   ')).toEqual([]);
  expect(findTranscriptMatches(segments, 'missing')).toEqual([]);
});

test('playback times remain readable for long and invalid durations', () => {
  expect(formatTime(65.9)).toBe('1:05');
  expect(formatTime(3600)).toBe('60:00');
  expect(formatTime(-1)).toBe('0:00');
  expect(formatTime(NaN)).toBe('0:00');
});
