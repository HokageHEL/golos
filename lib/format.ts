/**
 * Locale-aware formatting for money, durations, and dates.
 *
 * Everything the user reads goes through `Intl` rather than string
 * concatenation, so `uk` gets its own separators, unit names, and month names
 * without any call site knowing about it.
 */

/** Prices are integer US cents everywhere — see lib/billing/pricing.ts. */
export function formatMoney(cents: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    style: 'currency',
    currency: 'USD',
  }).format(cents / 100);
}

/** Rounds up: a 61-second file is charged as two minutes, so show two. */
export function formatMinutes(seconds: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: 'minute',
    unitDisplay: 'short',
  }).format(Math.ceil(seconds / 60));
}

export function formatMinuteCount(minutes: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: 'minute',
    unitDisplay: 'short',
  }).format(minutes);
}

export function formatDate(iso: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }).format(new Date(iso));
}
