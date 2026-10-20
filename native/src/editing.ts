import type { Segment } from './transcript/types';

export function replaceAll(segments: Segment[], find: string, replacement: string): Segment[] {
  if (!find) return segments;
  return segments.map(s => ({ ...s, text: s.text.split(find).join(replacement) }));
}

/** Split at a text offset; interpolated paragraph timing is not word alignment. */
export function splitParagraph(segments: Segment[], index: number, offset: number): Segment[] {
  const s = segments[index];
  if (!s || offset <= 0 || offset >= s.text.length || s.endMs - s.startMs < 2) return segments;
  const boundary = Math.max(s.startMs + 1, Math.min(s.endMs - 1, Math.round(s.startMs + (s.endMs - s.startMs) * offset / s.text.length)));
  return [...segments.slice(0, index), { ...s, endMs: boundary, text: s.text.slice(0, offset).trimEnd() },
    { ...s, startMs: boundary, text: s.text.slice(offset).trimStart() }, ...segments.slice(index + 1)];
}

/** Different speakers must be corrected explicitly before merging. */
export function mergeParagraph(segments: Segment[], index: number): Segment[] {
  const a = segments[index], b = segments[index + 1];
  if (!a || !b || a.speaker !== b.speaker) return segments;
  return [...segments.slice(0, index), { ...a, endMs: Math.max(a.endMs, b.endMs), text: a.text + ' ' + b.text }, ...segments.slice(index + 2)];
}

export type History = { past: Segment[][]; present: Segment[]; future: Segment[][] };
export function change(history: History, next: Segment[]): History {
  if (next === history.present) return history;
  return { past: [...history.past.slice(-99), history.present], present: next, future: [] };
}
export function undo(history: History): History {
  const previous = history.past.at(-1);
  return previous ? { past: history.past.slice(0, -1), present: previous, future: [history.present, ...history.future] } : history;
}
export function redo(history: History): History {
  const next = history.future[0];
  return next ? { past: [...history.past, history.present], present: next, future: history.future.slice(1) } : history;
}
