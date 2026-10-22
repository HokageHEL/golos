import type { Turn } from '../transcript/types';
import { MAX_SUBTITLE_MS, validateSubtitleOptions, type SubtitleOptions } from './options';

export type SubtitleCue = {
  startMs: number;
  endMs: number;
  lines: string[];
};

type GraphemeSegmenter = {
  segment(value: string): Iterable<{ segment: string }>;
};

const segmenter = (() => {
  const intl = Intl as typeof Intl & {
    Segmenter?: new (locales?: string | string[], options?: { granularity: 'grapheme' }) => GraphemeSegmenter;
  };
  return intl.Segmenter ? new intl.Segmenter(undefined, { granularity: 'grapheme' }) : null;
})();

/** Unicode-aware grapheme clusters; falls back without breaking surrogate pairs. */
export function graphemes(value: string): string[] {
  if (segmenter) return Array.from(segmenter.segment(value), part => part.segment);
  return Array.from(value);
}

function wrapLine(line: string, width: number): string[] {
  if (width <= 0) return [line];
  const words = line.trim().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const lines: string[] = [];
  let current = '';

  const pushLongToken = (word: string) => {
    const parts = graphemes(word);
    for (let index = 0; index < parts.length; index += width) {
      const chunk = parts.slice(index, index + width).join('');
      if (current) {
        lines.push(current);
        current = '';
      }
      if (index + width >= parts.length) current = chunk;
      else lines.push(chunk);
    }
  };

  for (const word of words) {
    const length = graphemes(word).length;
    if (length > width) {
      pushLongToken(word);
      continue;
    }
    if (!current) current = word;
    else if (graphemes(current).length + 1 + length <= width) current += ` ${word}`;
    else {
      lines.push(current);
      current = word;
    }
  }
  if (current) lines.push(current);
  return lines;
}

/**
 * Wrap subtitle text without splitting cues or inventing timestamps.
 * Existing hard line breaks are kept; blank lines are collapsed because they
 * would terminate an SRT/VTT cue. Width counts grapheme clusters, so combining
 * marks and emoji sequences stay intact, and overlong unbroken tokens are
 * chunked at grapheme boundaries.
 */
export function wrapSubtitleText(text: string, width: number): string[] {
  const source = text.replace(/\r\n?/g, '\n');
  if (width <= 0) return source.split('\n').filter(line => line.trim()).map(line => line.trim());
  return source
    .split('\n')
    .filter(line => line.trim())
    .flatMap(line => wrapLine(line, width));
}

function shiftedTimes(startMs: number, endMs: number, offsetMs: number): { startMs: number; endMs: number } {
  if (!Number.isSafeInteger(startMs) || !Number.isSafeInteger(endMs)) {
    throw new Error('Subtitle cue timestamps must be whole, finite milliseconds.');
  }
  if (startMs < 0 || endMs < 0) {
    throw new Error('Subtitle cue has a negative saved timestamp.');
  }
  const start = startMs + offsetMs;
  const end = endMs + offsetMs;
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end)) {
    throw new Error('Subtitle timestamp offset exceeds the safe timestamp range.');
  }
  if (start < 0 || end < 0) {
    throw new Error('Subtitle timestamp offset would produce negative cue times. Choose a smaller negative offset.');
  }
  if (start > MAX_SUBTITLE_MS || end > MAX_SUBTITLE_MS) {
    throw new Error('Subtitle timestamp offset exceeds the 99:59:59.999 subtitle limit.');
  }
  if (end <= start) {
    throw new Error('Subtitle cue has no positive duration. Timestamps are never invented to repair it.');
  }
  return { startMs: start, endMs: end };
}

/**
 * One cue per non-empty paragraph. Uniform offsets preserve order and overlap;
 * empty paragraphs are omitted rather than emitted as invalid cues. Overlapping
 * speech is deliberately kept instead of being clamped or dropped.
 */
export function subtitleCues(turns: Turn[], options: SubtitleOptions): SubtitleCue[] {
  const { offsetMs, maxLineLength } = validateSubtitleOptions(options);
  const cues: SubtitleCue[] = [];
  let previousStart = -1;
  for (const turn of turns) {
    if (!turn.text.trim()) continue;
    const times = shiftedTimes(turn.startMs, turn.endMs, offsetMs);
    if (times.startMs < previousStart) {
      throw new Error('Subtitle cue timestamps are out of order. Saved transcript order is never rewritten.');
    }
    previousStart = times.startMs;
    const lines = wrapSubtitleText(turn.text, maxLineLength);
    if (!lines.length) continue;
    cues.push({ ...times, lines });
  }
  return cues;
}
