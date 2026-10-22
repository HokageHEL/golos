import { expect, test } from 'vitest';
import { exportDocument } from '../document';
import { toSrt } from '../srt';
import { toVtt } from '../vtt';
import { graphemes, wrapSubtitleText } from '../subtitles';
import { MAX_SUBTITLE_MS } from '../options';
import type { Segment } from '../../transcript/types';

const segments: Segment[] = [
  { startMs: 0, endMs: 1000, speaker: 'Alice', text: 'One' },
  { startMs: 1000, endMs: 2000, speaker: 'Alice', text: 'Two' },
  { startMs: 2000, endMs: 3000, speaker: 'Bob', text: 'Three' },
  { startMs: 3000, endMs: 4000, speaker: null, text: 'Four' },
];

test('readable TXT options include or hide timestamps and speakers', () => {
  const shown = exportDocument('Ignored', segments, 'txt', {
    layout: { includeTimestamps: true, includeSpeakers: true, grouping: 'paragraph' },
  });
  expect(shown).toContain('[00:00:00.000 – 00:00:01.000] Alice: One');
  const hidden = exportDocument('Ignored', segments, 'txt', {
    layout: { includeTimestamps: false, includeSpeakers: false, grouping: 'paragraph' },
  });
  expect(hidden).toBe('One\n\nTwo\n\nThree\n\nFour');
});

test('adjacent same-speaker grouping merges only within a speaker turn', () => {
  expect(exportDocument('Ignored', segments, 'txt', {
    layout: { includeTimestamps: false, includeSpeakers: true, grouping: 'same-speaker' },
  })).toBe('Alice: One\nTwo\n\nBob: Three\n\nFour');
  // A later segment from an earlier speaker starts a new group.
  expect(exportDocument('Ignored', [
    { startMs: 0, endMs: 1000, speaker: 'A', text: 'One' },
    { startMs: 1000, endMs: 2000, speaker: 'A', text: 'Two' },
    { startMs: 2000, endMs: 3000, speaker: 'B', text: 'Three' },
    { startMs: 3000, endMs: 4000, speaker: 'A', text: 'Four' },
  ], 'txt', {
    layout: { includeTimestamps: false, includeSpeakers: false, grouping: 'same-speaker' },
  })).toBe('One\nTwo\n\nThree\n\nFour');
  // null breaks a group instead of being treated as a shared speaker.
  const unknown: Segment[] = [
    { startMs: 0, endMs: 1000, speaker: null, text: 'A' },
    { startMs: 1000, endMs: 2000, speaker: null, text: 'B' },
  ];
  expect(exportDocument('Ignored', unknown, 'txt', {
    layout: { includeTimestamps: false, includeSpeakers: false, grouping: 'same-speaker' },
  })).toBe('A\n\nB');
});

test('Markdown and HTML layouts omit hidden headers and preserve escaped text', () => {
  const markdown = exportDocument('Title', segments.slice(0, 3), 'md', {
    layout: { includeTimestamps: false, includeSpeakers: true, grouping: 'same-speaker' },
  });
  expect(markdown).toContain('**Alice**\n\nOne\nTwo');
  expect(markdown).not.toContain('00:00:00.000');
  const html = exportDocument('<Title>', segments.slice(0, 1), 'html', {
    layout: { includeTimestamps: false, includeSpeakers: false, grouping: 'paragraph' },
  });
  expect(html).toContain('<section><p>One</p></section>');
  expect(html).not.toContain('<header>');
  expect(html).toContain('&lt;Title&gt;');
});

test('CSV and JSON ignore layout options and retain canonical data', () => {
  const options = { layout: { includeTimestamps: false, includeSpeakers: false, grouping: 'same-speaker' as const } };
  const json = JSON.parse(exportDocument('Title', segments, 'json', options));
  expect(json.segments).toEqual(segments);
  const csv = exportDocument('Title', segments, 'csv', options);
  expect(csv).toContain('"Alice";"One"');
  expect(csv).toContain('"0";"1000"');
});

test('export options never mutate saved segments', () => {
  const original = segments.map(segment => ({ ...segment }));
  Object.freeze(segments[0]);
  exportDocument('Title', segments, 'txt', {
    layout: { includeTimestamps: true, includeSpeakers: false, grouping: 'same-speaker' },
  });
  exportDocument('Title', segments, 'srt', { subtitles: { offsetMs: 500, maxLineLength: 20 } });
  expect(segments).toEqual(original);
});

test('SRT offsets and wraps without changing one-cue-per-segment timing', () => {
  const srt = toSrt(segments, { offsetMs: 500, maxLineLength: 20 });
  expect(srt).toContain('1\n00:00:00,500 --> 00:00:01,500\nOne');
  expect(srt).toContain('4\n00:00:03,500 --> 00:00:04,500\nFour');
  expect(srt.match(/-->/g)).toHaveLength(4);
});

test('VTT offsets, escapes and wraps while keeping cue order', () => {
  const vtt = toVtt(
    [{ startMs: 500, endMs: 3600_000, speaker: null, text: '<hello> world' }],
    { offsetMs: -500, maxLineLength: 10 },
  );
  expect(vtt.startsWith('WEBVTT\n\n00:00:00.000 --> 00:59:59.500\n')).toBe(true);
  expect(vtt).toContain('&lt;hello&gt;');
  expect(vtt.match(/-->/g)).toHaveLength(1);
  const wrapped = toVtt(
    [{ startMs: 0, endMs: 1000, speaker: null, text: 'word and a verylongtoken' }],
    { offsetMs: 0, maxLineLength: 10 },
  );
  for (const line of wrapped.split('\n').filter(line => !line.includes('-->') && line !== 'WEBVTT' && line)) {
    expect(graphemes(line).length).toBeLessThanOrEqual(10);
  }
});

test('negative, nonfinite, fractional and overflowing offsets are rejected', () => {
  expect(() => toSrt([{ startMs: 100, endMs: 1000, speaker: null, text: 'A' }], { offsetMs: -101, maxLineLength: 0 }))
    .toThrow(/negative/);
  expect(() => toSrt([{ startMs: 0, endMs: 1000, speaker: null, text: 'A' }], { offsetMs: Number.NaN, maxLineLength: 0 }))
    .toThrow(/whole, finite/);
  expect(() => toSrt([{ startMs: 0, endMs: 1000, speaker: null, text: 'A' }], { offsetMs: Number.POSITIVE_INFINITY, maxLineLength: 0 }))
    .toThrow(/whole, finite/);
  expect(() => toSrt([{ startMs: 0, endMs: 1000, speaker: null, text: 'A' }], { offsetMs: 0.5, maxLineLength: 0 }))
    .toThrow(/whole, finite/);
  expect(() => toSrt([{ startMs: 0, endMs: 1000, speaker: null, text: 'A' }], { offsetMs: MAX_SUBTITLE_MS + 1, maxLineLength: 0 }))
    .toThrow(/99:59:59.999/);
  expect(() => toSrt([{ startMs: 0, endMs: 1000, speaker: null, text: 'A' }], { offsetMs: Number.MAX_SAFE_INTEGER, maxLineLength: 0 }))
    .toThrow(/safe timestamp range/);
});

test('invalid order and zero-duration cues are rejected instead of repaired', () => {
  expect(() => toSrt([
    { startMs: 2000, endMs: 3000, speaker: null, text: 'Later' },
    { startMs: 1000, endMs: 1500, speaker: null, text: 'Earlier' },
  ], { offsetMs: 0, maxLineLength: 0 })).toThrow(/out of order/);
  expect(() => toSrt([{ startMs: 1000, endMs: 1000, speaker: null, text: 'Empty time' }], { offsetMs: 0, maxLineLength: 0 }))
    .toThrow(/no positive duration/);
});

test('empty cues are omitted and overlapping speech is preserved', () => {
  const srt = toSrt([
    { startMs: 0, endMs: 2000, speaker: 'Alice', text: 'First speaker' },
    { startMs: 500, endMs: 1000, speaker: 'Bob', text: 'Overlapping reply' },
    { startMs: 1000, endMs: 1500, speaker: null, text: '   ' },
  ], { offsetMs: 0, maxLineLength: 0 });
  expect(srt).toContain('1\n00:00:00,000 --> 00:00:02,000\nFirst speaker');
  expect(srt).toContain('2\n00:00:00,500 --> 00:00:01,000\nOverlapping reply');
  expect(srt).not.toContain('3\n');
  // Overlap is kept; the second cue is not clamped to follow the first.
  expect(srt).toContain('00:00:00,500 --> 00:00:01,000');
});

test('Unicode-aware wrapping keeps grapheme clusters and chunks long tokens', () => {
  expect(wrapSubtitleText('The quick brown fox', 10)).toEqual(['The quick', 'brown fox']);
  expect(wrapSubtitleText('supercalifragilistic', 5)).toEqual(['super', 'calif', 'ragil', 'istic']);
  const family = '👩‍👩‍👧‍👦';
  const emoji = wrapSubtitleText(`${family}${family}`, 2);
  expect(emoji).toHaveLength(1);
  expect(graphemes(emoji[0])).toHaveLength(2);
  expect(emoji[0]).toContain(family);
  const combining = wrapSubtitleText('e\u0301e\u0301e\u0301', 2);
  expect(combining).toEqual(['e\u0301e\u0301', 'e\u0301']);
  expect(wrapSubtitleText('Привіт світе', 7)).toEqual(['Привіт', 'світе']);
});


test('custom subtitle exports omit empty speech even when a speaker label exists', () => {
  const input: Segment[] = [
    { startMs: 0, endMs: 1000, speaker: 'Silent speaker', text: ' \n\t' },
    { startMs: 1000, endMs: 2000, speaker: 'Alice', text: 'Spoken text' },
  ];
  const options = { offsetMs: 0, maxLineLength: 32 };
  for (const format of ['srt', 'vtt'] as const) {
    const output = exportDocument('Title', input, format, { subtitles: options });
    expect(output.match(/-->/g)).toHaveLength(1);
    expect(output).not.toContain('Silent speaker');
    expect(output).toContain('Alice: Spoken text');
  }
  expect(toVtt(input, options)).not.toContain('Silent speaker');
  expect(input[0].speaker).toBe('Silent speaker');
});
