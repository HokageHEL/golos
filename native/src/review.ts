import type { Segment } from './transcript/types';

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
