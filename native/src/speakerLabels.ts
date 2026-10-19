import type { Segment } from './transcript/types';
import { toSrt } from './export/srt';

export function displaySpeaker(speaker: string | null): string {
  return speaker?.replace(/^speaker_(\d+)$/, 'Speaker $1') ?? '';
}

/** Capture the group before editing so clearing/retyping keeps its membership. */
export function speakerEntryIndexes(segments: Segment[], index: number): number[] {
  const speaker = segments[index]?.speaker;
  if (speaker == null) return [index];
  return segments.flatMap((segment, n) => segment.speaker === speaker ? [n] : []);
}

export function renameSpeakerEntries(segments: Segment[], entries: readonly number[], label: string): Segment[] {
  const speaker = label.replace(/^Speaker (\d+)$/, 'speaker_$1') || null;
  const indexes = new Set(entries);
  return segments.map((segment, index) => indexes.has(index) ? { ...segment, speaker } : segment);
}

/** Preserve corrected speaker names in both native exports. */
export function exportWithSpeakers(segments: Segment[], format: 'txt' | 'srt'): string {
  const labeled = segments.map(segment => ({
    ...segment,
    text: segment.speaker ? `${displaySpeaker(segment.speaker)}: ${segment.text}` : segment.text,
  }));
  return format === 'srt' ? toSrt(labeled) : labeled.map(segment => segment.text).join('\n\n');
}
