import { expect, test } from 'vitest';
import { anonKey } from '../identity';

test('is stable for the same visitor', () => {
  expect(anonKey('1.2.3.4', 'fp-abc')).toBe(anonKey('1.2.3.4', 'fp-abc'));
});

test('changes when either input changes', () => {
  const base = anonKey('1.2.3.4', 'fp-abc');
  expect(anonKey('5.6.7.8', 'fp-abc')).not.toBe(base);
  expect(anonKey('1.2.3.4', 'fp-xyz')).not.toBe(base);
});

test('does not leak the raw ip', () => {
  expect(anonKey('1.2.3.4', 'fp-abc')).not.toContain('1.2.3.4');
});

test('separator prevents collisions between shifted inputs', () => {
  // Without a delimiter, "ab"+"c" and "a"+"bc" would hash identically.
  expect(anonKey('ab', 'c')).not.toBe(anonKey('a', 'bc'));
});
