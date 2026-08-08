import type { Segment, Turn } from './types';

export function toTurns(segments: Segment[]): Turn[] {
  const turns: Turn[] = [];

  for (const s of segments) {
    const last = turns[turns.length - 1];
    if (last && last.speaker === s.speaker) {
      last.text = `${last.text} ${s.text}`.trim();
      last.endMs = s.endMs;
    } else {
      turns.push({ startMs: s.startMs, endMs: s.endMs, speaker: s.speaker, text: s.text });
    }
  }

  return turns;
}

export function wordCount(text: string): number {
  return text.trim().split(/\s+/).filter(Boolean).length;
}
