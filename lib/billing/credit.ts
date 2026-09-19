import { TOPUP_OPTIONS_CENTS } from './pricing';

/**
 * The subset of a Polar order this module reasons about. Declared here rather
 * than imported from the SDK so the rules stay testable without constructing a
 * full 40-field Order, and so an SDK model change surfaces as a type error at
 * the one call site instead of silently altering the money rules.
 */
export type PaidOrder = {
  id: string;
  paid: boolean;
  currency: string;
  netAmount: number;
  billingReason: string;
  metadata: Record<string, unknown>;
  customer: { externalId?: string | null };
};

/** A row for the append-only ledger. `external_id` is the exactly-once key. */
export type CreditEntry = {
  user_id: string;
  delta_cents: number;
  reason: string;
  job_id: null;
  external_id: string;
};

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * Parse `POLAR_TOPUP_PRODUCTS` — `"1000:prod_a,2500:prod_b,5000:prod_c"`.
 *
 * One env var rather than three keeps the mapping in a single place, so adding
 * a top-up tier is a change to `pricing.ts` plus this string, nothing else.
 * Malformed entries are dropped instead of throwing: a typo in one tier should
 * disable that tier, not take down every route that reads the config.
 */
export function parseTopUpProducts(raw: string | undefined): Map<number, string> {
  const map = new Map<number, string>();
  if (!raw) return map;

  for (const pair of raw.split(',')) {
    const [centsPart, productPart] = pair.split(':');
    if (centsPart === undefined || productPart === undefined) continue;

    const cents = Number(centsPart.trim());
    const productId = productPart.trim();
    if (!Number.isInteger(cents) || cents <= 0 || productId === '') continue;

    map.set(cents, productId);
  }

  return map;
}

/**
 * Resolve the Polar product for a requested top-up, or null if we do not sell
 * it. The amount arrives from the browser, so it is checked against the price
 * list first — a configured-but-unlisted product is not purchasable.
 */
export function topUpProductId(raw: string | undefined, cents: number): string | null {
  if (!TOPUP_OPTIONS_CENTS.includes(cents as (typeof TOPUP_OPTIONS_CENTS)[number])) return null;
  return parseTopUpProducts(raw).get(cents) ?? null;
}

/**
 * Decide what a paid Polar order adds to the ledger, or null to ignore it.
 *
 * Pure on purpose: every reason to refuse money — wrong currency, unpaid,
 * unidentifiable buyer — is a rule that must be readable and testable without
 * a database or a webhook. Returning null means "acknowledge and do nothing",
 * never "retry", because a retry would not change any of these answers.
 */
export function creditForOrder(order: PaidOrder): CreditEntry | null {
  if (!order.paid) return null;

  // The ledger stores USD cents with no currency column, so an amount in any
  // other currency cannot be recorded — only mis-recorded.
  if (order.currency.toLowerCase() !== 'usd') return null;

  // Only one-off purchases become credit. A subscription order grants plan
  // minutes instead; crediting it here as well would pay the user twice.
  if (order.billingReason !== 'purchase') return null;

  // net_amount is the price after discounts and BEFORE tax. Polar is the
  // merchant of record, so total_amount includes VAT that is not ours.
  if (!Number.isInteger(order.netAmount) || order.netAmount <= 0) return null;

  const userId = order.metadata['user_id'] ?? order.customer.externalId;
  if (typeof userId !== 'string' || !UUID.test(userId)) return null;

  return {
    user_id: userId,
    delta_cents: order.netAmount,
    reason: 'topup',
    job_id: null,
    // Keyed by the ORDER, not the delivery or the event: order.paid may be
    // redelivered, and order.created/order.updated describe the same money.
    // One order can only ever credit once.
    external_id: `polar:order:${order.id}`,
  };
}
