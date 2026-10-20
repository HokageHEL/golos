import type { Segment } from '../transcript/types';
import { formatSrtTime } from './srt';
import { displaySpeaker } from '../speakerLabels';
import type { SubtitleOptions } from './options';
import { subtitleCues } from './subtitles';

const escape = (text: string) => text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');

/** Legacy path (no options) retains the existing exact output. */
export function toVtt(segments: Segment[]): string;
export function toVtt(segments: Segment[], options: SubtitleOptions): string;
export function toVtt(segments: Segment[], options?: SubtitleOptions): string {
  if (!options) {
    return 'WEBVTT\n\n' + segments.map(s => {
      const text = s.speaker ? displaySpeaker(s.speaker) + ': ' + s.text : s.text;
      return `${formatSrtTime(s.startMs).replace(',', '.')} --> ${formatSrtTime(s.endMs).replace(',', '.')}\n${escape(text.replace(/\r?\n\s*\r?\n/g, '\n'))}\n`;
    }).join('\n');
  }
  const turns = segments.map(segment => ({
    ...segment,
    text: segment.speaker ? `${displaySpeaker(segment.speaker)}: ${segment.text}` : segment.text,
  }));
  return 'WEBVTT\n\n' + subtitleCues(turns, options)
    .map(cue =>
      `${formatSrtTime(cue.startMs).replace(',', '.')} --> ${formatSrtTime(cue.endMs).replace(',', '.')}\n${cue.lines.map(escape).join('\n')}\n`,
    )
    .join('\n');
}
