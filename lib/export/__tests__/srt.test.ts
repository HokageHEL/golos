import { expect, test } from 'vitest';
import { formatSrtTime, toSrt } from '../srt';

test('formats timestamps as HH:MM:SS,mmm', () => {
  expect(formatSrtTime(0)).toBe('00:00:00,000');
  expect(formatSrtTime(1)).toBe('00:00:00,001');
  expect(formatSrtTime(61_500)).toBe('00:01:01,500');
  expect(formatSrtTime(3_661_042)).toBe('01:01:01,042');
});

test('numbers cues from 1 and separates them with a blank line', () => {
  const srt = toSrt([
    { startMs: 0, endMs: 1500, speaker: 'A', text: 'Hello.' },
    { startMs: 1500, endMs: 3000, speaker: 'B', text: 'Hi.' },
  ]);

  expect(srt).toBe(
    '1\n00:00:00,000 --> 00:00:01,500\nHello.\n\n' +
      '2\n00:00:01,500 --> 00:00:03,000\nHi.\n',
  );
});

test('empty input yields an empty string', () => {
  expect(toSrt([])).toBe('');
});
