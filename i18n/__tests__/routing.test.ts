import { expect, test } from 'vitest';
import en from '../../messages/en.json';
import uk from '../../messages/uk.json';

function flatten(obj: Record<string, unknown>, prefix = ''): string[] {
  return Object.entries(obj).flatMap(([k, v]) =>
    typeof v === 'object' && v !== null
      ? flatten(v as Record<string, unknown>, `${prefix}${k}.`)
      : [`${prefix}${k}`],
  );
}

test('en and uk have identical key sets', () => {
  expect(flatten(uk).sort()).toEqual(flatten(en).sort());
});
