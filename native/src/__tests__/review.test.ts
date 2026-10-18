import { expect, test } from 'vitest';
import type { Segment } from '../transcript/types';
import { findTranscriptMatches } from '../review';

const segments: Segment[] = [
  { startMs: 500, endMs: 2000, speaker: null, text: 'Привіт, Київ! Привіт.' },
  { startMs: 3000, endMs: 5000, speaker: null, text: 'Check C++ and [notes].' },
];

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
