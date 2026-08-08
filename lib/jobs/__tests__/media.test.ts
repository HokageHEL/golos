import { expect, test } from 'vitest';
import { isSupportedAudio, storagePathFor } from '../media';

test('accepts the formats the landing page advertises', () => {
  for (const type of ['audio/mpeg', 'audio/wav', 'audio/x-m4a', 'audio/mp4', 'video/mp4']) {
    expect(isSupportedAudio(type)).toBe(true);
  }
});

test('rejects everything else', () => {
  for (const type of ['application/pdf', 'image/png', 'text/plain', '']) {
    expect(isSupportedAudio(type)).toBe(false);
  }
});

test('storage paths are namespaced by job and keep the extension', () => {
  const path = storagePathFor('job-1', 'meeting notes.mp3');
  expect(path).toBe('job-1/source.mp3');
});

test('storage paths fall back to .bin for extensionless names', () => {
  expect(storagePathFor('job-1', 'recording')).toBe('job-1/source.bin');
});
