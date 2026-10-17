import type { Segment } from './transcript/types';

/** Playback speeds offered by the audio player. `1` is the natural rate. */
export const PLAYBACK_RATES = [0.75, 1, 1.25, 1.5, 2] as const;

export type PlaybackRate = (typeof PLAYBACK_RATES)[number];

/** An external "move the playhead here" request. The nonce makes repeated
 * seeks to the same millisecond observable. */
export type SeekRequest = { ms: number; nonce: number };

/**
 * Format milliseconds as a clock. Under an hour this is `m:ss` (matching the
 * transcript timecodes); longer recordings get `h:mm:ss`.
 */
export function formatClock(ms: number): string {
  const safe = Number.isFinite(ms) && ms > 0 ? Math.floor(ms) : 0;
  const totalSeconds = Math.floor(safe / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  }
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

/**
 * Index of the segment that covers `timeMs`, or `null` when the playhead sits
 * in a gap (or before/after every segment).
 *
 * Boundaries are half-open: a segment covers `[startMs, endMs)`, so the very
 * first millisecond of the next segment is no longer the previous one, and
 * zero-length segments are never active. When segments overlap, the one that
 * started most recently wins, which is what a listener expects as playback
 * advances into the newer segment. The scan does not rely on segment order.
 */
export function activeSegmentIndex(
  segments: readonly Segment[],
  timeMs: number,
): number | null {
  if (!Number.isFinite(timeMs) || timeMs < 0) return null;
  let best: number | null = null;
  for (let index = 0; index < segments.length; index += 1) {
    const segment = segments[index];
    if (timeMs >= segment.startMs && timeMs < segment.endMs) {
      if (best === null || segment.startMs >= segments[best].startMs) {
        best = index;
      }
    }
  }
  return best;
}

/**
 * Clamp a seek target to the recording. A non-finite target falls back to the
 * start, and a missing or still-unknown duration leaves the upper bound open so
 * an early seek is not pinned to zero.
 */
export function clampSeek(ms: number, durationMs?: number): number {
  const lower = Number.isFinite(ms) ? Math.max(0, ms) : 0;
  if (durationMs === undefined || !Number.isFinite(durationMs) || durationMs <= 0) {
    return lower;
  }
  return Math.min(lower, durationMs);
}

/** Only the offered rates are allowed; anything else falls back to 1x. */
export function normalizeRate(rate: number): PlaybackRate {
  return (PLAYBACK_RATES as readonly number[]).includes(rate)
    ? (rate as PlaybackRate)
    : 1;
}
