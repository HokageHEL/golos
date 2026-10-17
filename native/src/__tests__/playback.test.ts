import { expect, test } from 'vitest';
import {
  PLAYBACK_RATES,
  activeSegmentIndex,
  clampSeek,
  formatClock,
  normalizeRate,
} from '../playback';
import type { Segment } from '../../../lib/transcript/types';

const seg = (startMs: number, endMs: number, text = 'text'): Segment => ({
  startMs,
  endMs,
  speaker: null,
  text,
});

test('selects the segment that covers the playhead', () => {
  const segments = [seg(0, 1000), seg(1000, 2500), seg(3000, 4000)];

  expect(activeSegmentIndex(segments, 0)).toBe(0);
  expect(activeSegmentIndex(segments, 999)).toBe(0);
  expect(activeSegmentIndex(segments, 1000)).toBe(1);
  expect(activeSegmentIndex(segments, 2499)).toBe(1);
  expect(activeSegmentIndex(segments, 3500)).toBe(2);
});

test('segment boundaries are half-open, so a shared timestamp switches segments', () => {
  const segments = [seg(0, 1000), seg(1000, 2000)];

  // The end of one segment is already the start of the next.
  expect(activeSegmentIndex(segments, 999)).toBe(0);
  expect(activeSegmentIndex(segments, 1000)).toBe(1);
  // The very end of the recording belongs to nobody.
  expect(activeSegmentIndex(segments, 2000)).toBeNull();
});

test('gaps between segments have no highlight', () => {
  const segments = [seg(0, 1000), seg(3000, 4000)];

  expect(activeSegmentIndex(segments, 1500)).toBeNull();
  expect(activeSegmentIndex(segments, 2999)).toBeNull();
});

test('an overlap resolves to the segment that started most recently', () => {
  const segments = [seg(0, 5000), seg(3000, 8000), seg(6000, 9000)];

  expect(activeSegmentIndex(segments, 1000)).toBe(0);
  expect(activeSegmentIndex(segments, 3500)).toBe(1);
  expect(activeSegmentIndex(segments, 7000)).toBe(2);
  // A tie on the start time is deterministic: the later segment wins.
  expect(activeSegmentIndex([seg(0, 5000), seg(0, 5000)], 1000)).toBe(1);
});

test('does not require the segments to be pre-sorted', () => {
  const segments = [seg(4000, 5000), seg(0, 1000), seg(2000, 3000)];

  expect(activeSegmentIndex(segments, 500)).toBe(1);
  expect(activeSegmentIndex(segments, 2500)).toBe(2);
  expect(activeSegmentIndex(segments, 4500)).toBe(0);
});

test('zero-length segments are never active and empty input has no highlight', () => {
  expect(activeSegmentIndex([seg(1000, 1000)], 1000)).toBeNull();
  expect(activeSegmentIndex([], 0)).toBeNull();
});

test('a playhead before the start or out of range has no highlight', () => {
  const segments = [seg(500, 1000)];

  expect(activeSegmentIndex(segments, -1)).toBeNull();
  expect(activeSegmentIndex(segments, 0)).toBeNull();
  expect(activeSegmentIndex(segments, 1001)).toBeNull();
  expect(activeSegmentIndex(segments, Number.NaN)).toBeNull();
});

test('seeking is clamped, unknown durations stay open, and non-finite targets reset', () => {
  expect(clampSeek(5000, 4000)).toBe(4000);
  expect(clampSeek(-100, 4000)).toBe(0);
  expect(clampSeek(1500, 4000)).toBe(1500);
  expect(clampSeek(1500)).toBe(1500);
  expect(clampSeek(1500, 0)).toBe(1500);
  expect(clampSeek(Number.NaN, 4000)).toBe(0);
  expect(clampSeek(Number.POSITIVE_INFINITY, 4000)).toBe(0);
});

test('clock formatting matches the transcript timecodes and adds hours when needed', () => {
  expect(formatClock(0)).toBe('0:00');
  expect(formatClock(65_000)).toBe('1:05');
  expect(formatClock(3_599_000)).toBe('59:59');
  expect(formatClock(3_600_000)).toBe('1:00:00');
  expect(formatClock(-5)).toBe('0:00');
  expect(formatClock(Number.NaN)).toBe('0:00');
});

test('only the offered playback rates are accepted', () => {
  for (const rate of PLAYBACK_RATES) {
    expect(normalizeRate(rate)).toBe(rate);
  }
  expect(normalizeRate(3)).toBe(1);
  expect(normalizeRate(0)).toBe(1);
  expect(normalizeRate(Number.NaN)).toBe(1);
});
