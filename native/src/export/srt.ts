import type { Turn } from '../transcript/types';
import type { SubtitleOptions } from './options';
import { subtitleCues } from './subtitles';

const pad = (n: number, width: number) => String(n).padStart(width, '0');

export function formatSrtTime(ms: number): string {
  const hours = Math.floor(ms / 3_600_000);
  const minutes = Math.floor((ms % 3_600_000) / 60_000);
  const seconds = Math.floor((ms % 60_000) / 1000);
  const millis = Math.floor(ms % 1000);
  return `${pad(hours, 2)}:${pad(minutes, 2)}:${pad(seconds, 2)},${pad(millis, 3)}`;
}

/** Legacy path (no options) retains the existing exact output. */
function legacySrt(turns: Turn[]): string {
  return turns
    .map(
      (t, i) =>
        `${i + 1}\n${formatSrtTime(t.startMs)} --> ${formatSrtTime(t.endMs)}\n${t.text.replace(/\r\n?/g, '\n').split('\n').filter(line => line.trim()).join('\n')}\n`,
    )
    .join('\n');
}

/**
 * Build SRT cues. With options, cues are validated, offset uniformly, and
 * wrapped without changing cue count or inventing word timings.
 */
export function toSrt(turns: Turn[], options?: SubtitleOptions): string {
  if (!options) return legacySrt(turns);
  return subtitleCues(turns, options)
    .map((cue, index) =>
      `${index + 1}\n${formatSrtTime(cue.startMs)} --> ${formatSrtTime(cue.endMs)}\n${cue.lines.join('\n')}\n`,
    )
    .join('\n');
}
