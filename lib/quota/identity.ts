import { createHash } from 'node:crypto';

/**
 * Identifies an anonymous visitor without storing anything personal.
 * IP alone is not enough: mobile carriers put thousands of people behind one
 * NAT address, so an IP-only rule blocks paying users and stops nobody.
 */
export function anonKey(ip: string, fingerprint: string): string {
  return createHash('sha256').update(`${ip} ${fingerprint}`).digest('hex');
}
