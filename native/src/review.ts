import type { Segment } from '../../lib/transcript/types';

export function formatTime(seconds: number): string {
  const whole = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
}

export function segmentAtTime(segments: Segment[], seconds: number): number {
  const ms = seconds * 1000;
  let low = 0;
  let high = segments.length - 1;
  let candidate = -1;
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    if (segments[middle].startMs <= ms) { candidate = middle; low = middle + 1; }
    else high = middle - 1;
  }
  return candidate >= 0 && ms < segments[candidate].endMs ? candidate : -1;
}

export type TranscriptMatch = { segmentIndex: number; start: number; end: number };

export function findTranscriptMatches(segments: Segment[], query: string): TranscriptMatch[] {
  const term = query.trim();
  if (!term) return [];
  const pattern = new RegExp(term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'giu');
  return segments.flatMap((segment, segmentIndex) =>
    Array.from(segment.text.matchAll(pattern), match => ({
      segmentIndex, start: match.index, end: match.index + match[0].length,
    })),
  );
}
