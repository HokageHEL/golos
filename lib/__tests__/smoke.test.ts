import { expect, test } from 'vitest';
import { add } from '@/lib/utils';

test('test runner works', () => {
  expect(1 + 1).toBe(2);
});

test('path alias works', () => {
  expect(add(2, 3)).toBe(5);
});
