import { expect, test } from 'vitest';
import { displaySpeaker, exportWithSpeakers, speakerEntryIndexes, renameSpeakerEntries } from '../speakerLabels';

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

test('renaming a recurring speaker updates every occurrence and preserves transcript content', () => {
  const original = [...segments, { startMs: 3000, endMs: 4000, speaker: 'speaker_1', text: 'Again' }];
  const renamed = renameSpeakerEntries(original, speakerEntryIndexes(original, 3), 'Alex');
  expect(renamed.map(segment => segment.speaker)).toEqual(['Alex', 'Anna', null, 'Alex']);
  expect(renamed.map(({ startMs, endMs, text }) => ({ startMs, endMs, text })))
    .toEqual(original.map(({ startMs, endMs, text }) => ({ startMs, endMs, text })));
  expect(original[0].speaker).toBe('speaker_1');
  expect(exportWithSpeakers(renamed, 'txt')).toBe('Alex: Hello\n\nAnna: Hi\n\nUnassigned\n\nAlex: Again');
  expect(renameSpeakerEntries(renamed, speakerEntryIndexes(renamed, 0), 'Sam').map(segment => segment.speaker))
    .toEqual(['Sam', 'Anna', null, 'Sam']);
});

test('clearing and retyping a name keeps the original group even through another speaker name', () => {
  const original = [...segments, { ...segments[0], text: 'Again' }];
  const entries = speakerEntryIndexes(original, 0);
  let draft = renameSpeakerEntries(original, entries, '');
  expect(draft.map(segment => segment.speaker)).toEqual([null, 'Anna', null, null]);
  draft = renameSpeakerEntries(draft, entries, 'Anna');
  draft = renameSpeakerEntries(draft, entries, 'Annabelle');
  expect(draft.map(segment => segment.speaker)).toEqual(['Annabelle', 'Anna', null, 'Annabelle']);
});

test('assigning an unknown entry leaves other unknown entries alone and preserves numbered labels', () => {
  const original = [...segments, { ...segments[2], text: 'Also unknown' }];
  const renamed = renameSpeakerEntries(original, speakerEntryIndexes(original, 2), 'Speaker 2');
  expect(renamed.map(segment => segment.speaker)).toEqual(['speaker_1', 'Anna', 'speaker_2', null]);
});
