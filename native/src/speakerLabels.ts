import type { Segment } from '../../lib/transcript/types';
import { toSrt } from '../../lib/export/srt';

export function displaySpeaker(speaker: string | null): string {
  return speaker?.replace(/^speaker_(\d+)$/, 'Speaker $1') ?? '';
}

/** Preserve corrected speaker names in both native exports. */
export function exportWithSpeakers(segments: Segment[], format: 'txt' | 'srt'): string {
  const labeled = segments.map(segment => ({
    ...segment,
    text: segment.speaker ? `${displaySpeaker(segment.speaker)}: ${segment.text}` : segment.text,
  }));
  return format === 'srt' ? toSrt(labeled) : labeled.map(segment => segment.text).join('\n\n');
}
