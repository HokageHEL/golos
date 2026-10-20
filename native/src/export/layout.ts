import type { Segment } from '../transcript/types';
import { displaySpeaker } from '../speakerLabels';
import type { SpeakerGrouping } from './options';

export type TranscriptBlock = {
  startMs: number;
  endMs: number;
  speaker: string | null;
  text: string;
};

/** Format a paragraph timestamp without inventing sub-paragraph precision. */
export function timestamp(ms: number): string {
  const value = Math.max(0, Math.round(ms));
  return `${String(Math.floor(value / 3_600_000)).padStart(2, '0')}:${String(Math.floor(value / 60_000) % 60).padStart(2, '0')}:${String(Math.floor(value / 1000) % 60).padStart(2, '0')}.${String(value % 1000).padStart(3, '0')}`;
}

function appendText(left: string, right: string): string {
  if (!left) return right;
  if (!right) return left;
  return `${left}\n${right}`;
}

/**
 * Group paragraphs without mutating saved segments.
 *
 * `paragraph` keeps one block per segment. `same-speaker` combines only
 * directly adjacent segments with the same non-null speaker; an unassigned
 * segment, a speaker change, or the end of the transcript always closes a
 * block. The block end is the latest segment end so overlapping speech is kept.
 */
export function transcriptBlocks(segments: Segment[], grouping: SpeakerGrouping): TranscriptBlock[] {
  const blocks: TranscriptBlock[] = [];
  for (const segment of segments) {
    const previous = blocks[blocks.length - 1];
    if (
      grouping === 'same-speaker'
      && previous
      && segment.speaker !== null
      && previous.speaker === segment.speaker
    ) {
      previous.text = appendText(previous.text, segment.text);
      previous.endMs = Math.max(previous.endMs, segment.endMs);
      continue;
    }
    blocks.push({
      startMs: segment.startMs,
      endMs: segment.endMs,
      speaker: segment.speaker,
      text: segment.text,
    });
  }
  return blocks;
}

export function speakerLabel(speaker: string | null): string {
  return displaySpeaker(speaker);
}

/** Header used by TXT; omitted entirely when both options are hidden. */
export function plainTimestampHeader(block: TranscriptBlock): string {
  return `[${timestamp(block.startMs)} – ${timestamp(block.endMs)}]`;
}

export function markdownHeader(block: TranscriptBlock, includeTimestamps: boolean, includeSpeakers: boolean): string {
  const parts: string[] = [];
  if (includeTimestamps) parts.push(`${timestamp(block.startMs)} – ${timestamp(block.endMs)}`);
  if (includeSpeakers) {
    const label = speakerLabel(block.speaker);
    if (label) parts.push(label);
  }
  return parts.join(' · ');
}
