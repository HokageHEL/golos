// @vitest-environment jsdom
import { beforeEach, expect, test, vi } from 'vitest';
import { defaultPreferences, readPreferences, savePreferences } from '../preferences';

const key = 'golos.transcription-preferences';

beforeEach(() => localStorage.clear());

test('preferences survive a fresh read and reject unsupported values', () => {
  savePreferences({ language: 'uk', autoSpeakers: false, speakerCount: 3 });
  expect(readPreferences()).toEqual({ language: 'uk', autoSpeakers: false, speakerCount: 3 });

  localStorage.setItem(key, JSON.stringify({ language: 'fr', autoSpeakers: 'yes', speakerCount: 7 }));
  expect(readPreferences()).toEqual(defaultPreferences);
});

test('malformed and unavailable storage fall back to defaults', () => {
  localStorage.setItem(key, '{broken');
  expect(readPreferences()).toEqual(defaultPreferences);

  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('storage blocked'); });
  expect(readPreferences()).toEqual(defaultPreferences);
});
