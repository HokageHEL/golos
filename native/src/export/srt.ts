import type { Turn } from '../transcript/types';

const pad = (n: number, width: number) => String(n).padStart(width, '0');

export function formatSrtTime(ms: number): string {
  const hours = Math.floor(ms / 3_600_000);
  const minutes = Math.floor((ms % 3_600_000) / 60_000);
  const seconds = Math.floor((ms % 60_000) / 1000);
  const millis = Math.floor(ms % 1000);
  return `${pad(hours, 2)}:${pad(minutes, 2)}:${pad(seconds, 2)},${pad(millis, 3)}`;
}

export function toSrt(turns: Turn[]): string {
  return turns
    .map(
      (t, i) =>
        `${i + 1}\n${formatSrtTime(t.startMs)} --> ${formatSrtTime(t.endMs)}\n${t.text}\n`,
    )
    .join('\n');
}
