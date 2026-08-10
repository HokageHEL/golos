import { expect, test } from 'vitest';
import {
  beginUpload,
  reportFailure,
  reportProgress,
  subscribeToUpload,
  uploadState,
} from '../upload-registry';

test('an unknown job has no state, so the page can tell "not tracked" from "0%"', () => {
  expect(uploadState('never-seen')).toBeUndefined();
});

test('progress is readable after the component that reported it is gone', () => {
  beginUpload('job-a');
  reportProgress('job-a', 0.42);

  // No subscriber at all — this is the hard-navigation case where the job
  // page mounts after the dropzone already reported.
  expect(uploadState('job-a')).toEqual({ fraction: 0.42, failure: null });
});

test('subscribers are notified on every publish', () => {
  const seen: (number | undefined)[] = [];
  const unsubscribe = subscribeToUpload('job-b', () => seen.push(uploadState('job-b')?.fraction));

  beginUpload('job-b');
  reportProgress('job-b', 0.5);
  reportProgress('job-b', 1);

  expect(seen).toEqual([0, 0.5, 1]);
  unsubscribe();
});

test('unsubscribing stops notifications', () => {
  let calls = 0;
  const unsubscribe = subscribeToUpload('job-c', () => (calls += 1));
  beginUpload('job-c');
  unsubscribe();
  reportProgress('job-c', 0.9);

  expect(calls).toBe(1);
});

test('each publish is a new object, or useSyncExternalStore would skip the render', () => {
  beginUpload('job-d');
  const first = uploadState('job-d');
  reportProgress('job-d', 0.1);

  expect(uploadState('job-d')).not.toBe(first);
});

test('failure keeps the fraction reached, so the page can say how far it got', () => {
  beginUpload('job-e');
  reportProgress('job-e', 0.75);
  reportFailure('job-e', 'upload');

  expect(uploadState('job-e')).toEqual({ fraction: 0.75, failure: 'upload' });
});

test('a start failure is distinguishable from an upload failure', () => {
  beginUpload('job-f');
  reportProgress('job-f', 1);
  reportFailure('job-f', 'start');

  expect(uploadState('job-f')?.failure).toBe('start');
});

test('subscribers of one job are not woken by another', () => {
  let calls = 0;
  const unsubscribe = subscribeToUpload('job-g', () => (calls += 1));
  beginUpload('job-h');
  reportProgress('job-h', 0.3);

  expect(calls).toBe(0);
  unsubscribe();
});
