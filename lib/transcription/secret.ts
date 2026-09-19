import { timingSafeEqual } from 'node:crypto';

/**
 * Constant-time secret comparison for webhook auth headers.
 *
 * The empty-secret guard is the load-bearing part: two empty buffers compare
 * equal, so a provider constructed with an unset env var would accept any
 * caller that sends the header with no value. Same reasoning as the cron
 * route — an unset secret must lock the endpoint, not open it.
 */
export function secretMatches(provided: string | null, expected: string | undefined): boolean {
  if (!provided || !expected) return false;

  const a = Buffer.from(provided);
  const b = Buffer.from(expected);
  return a.length === b.length && timingSafeEqual(a, b);
}
