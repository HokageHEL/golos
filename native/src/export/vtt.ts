import type { Segment } from '../transcript/types';
import { formatSrtTime } from './srt';
import { displaySpeaker } from '../speakerLabels';
const escape = (text: string) => text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replace(/\r?\n\s*\r?\n/g, '\n');
export function toVtt(segments: Segment[]): string {
  return 'WEBVTT\n\n' + segments.map(s => `${formatSrtTime(s.startMs).replace(',', '.')} --> ${formatSrtTime(s.endMs).replace(',', '.')}\n${escape(s.speaker ? displaySpeaker(s.speaker) + ': ' + s.text : s.text)}\n`).join('\n');
}
