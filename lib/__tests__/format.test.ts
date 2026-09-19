import { expect, test } from 'vitest';
import { formatDate, formatMinuteCount, formatMinutes, formatMoney } from '../format';
import { PAYG_RATE_CENTS, SUBSCRIPTION } from '../billing/pricing';

test('renders cents as currency, not raw integers', () => {
  expect(formatMoney(SUBSCRIPTION.priceCents, 'en')).toBe('$9.00');
  expect(formatMoney(PAYG_RATE_CENTS, 'en')).toBe('$0.05');
  expect(formatMoney(0, 'en')).toBe('$0.00');
});

test('formats money for uk without hardcoding separators', () => {
  const formatted = formatMoney(250000, 'uk');
  // Українська локаль ставить нерозривний пробіл — головне, що не "2500.00".
  expect(formatted).toContain('2');
  expect(formatted).not.toBe('$2500.00');
});

test('rounds partial minutes up, matching how they are billed', () => {
  expect(formatMinutes(61, 'en')).toBe('2 min');
  expect(formatMinutes(60, 'en')).toBe('1 min');
  expect(formatMinutes(0, 'en')).toBe('0 min');
});

test('formats a plan minute allowance', () => {
  expect(formatMinuteCount(SUBSCRIPTION.minutesIncluded, 'en')).toBe('300 min');
});

test('formats dates per locale rather than by string slicing', () => {
  const iso = '2026-03-14T10:30:00.000Z';
  expect(formatDate(iso, 'en')).toBe('Mar 14, 2026');
  expect(formatDate(iso, 'uk')).not.toBe(formatDate(iso, 'en'));
});
