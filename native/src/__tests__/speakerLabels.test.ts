import { expect, test } from 'vitest';
import { displaySpeaker, exportWithSpeakers } from '../speakerLabels';

const segments = [
  { startMs: 0, endMs: 1000, speaker: 'speaker_1', text: 'Hello' },
  { startMs: 1000, endMs: 2000, speaker: 'Anna', text: 'Hi' },
  { startMs: 2500, endMs: 3000, speaker: null, text: 'Unassigned' },
];
test('native exports preserve corrected names and leave unknown speech unlabelled', () => {
  expect(displaySpeaker('speaker_1')).toBe('Speaker 1');
  expect(displaySpeaker('Anna')).toBe('Anna');
  expect(exportWithSpeakers(segments, 'txt')).toBe('Speaker 1: Hello\n\nAnna: Hi\n\nUnassigned');
  expect(exportWithSpeakers(segments, 'srt')).toContain('00:00:01,000 --> 00:00:02,000\nAnna: Hi');
  expect(segments[1].text).toBe('Hi');
});
