import { describe, expect, it } from 'vitest';
import { creditForOrder, topUpProductId, parseTopUpProducts, type PaidOrder } from '../credit';

const USER = '5d1b1f6e-2b7f-4a1d-9a3c-0f1e2d3c4b5a';

function order(overrides: Partial<PaidOrder> = {}): PaidOrder {
  return {
    id: 'ord_123',
    paid: true,
    currency: 'usd',
    netAmount: 1000,
    billingReason: 'purchase',
    metadata: { user_id: USER },
    customer: { externalId: USER },
    ...overrides,
  };
}

describe('parseTopUpProducts', () => {
  it('reads a cents:productId list', () => {
    const map = parseTopUpProducts('1000:prod_a,2500:prod_b');
    expect(map.get(1000)).toBe('prod_a');
    expect(map.get(2500)).toBe('prod_b');
  });

  it('tolerates whitespace and a trailing comma', () => {
    const map = parseTopUpProducts(' 1000 : prod_a , 2500:prod_b, ');
    expect(map.get(1000)).toBe('prod_a');
    expect(map.get(2500)).toBe('prod_b');
    expect(map.size).toBe(2);
  });

  it('is empty when unset, rather than throwing at import time', () => {
    expect(parseTopUpProducts(undefined).size).toBe(0);
    expect(parseTopUpProducts('').size).toBe(0);
  });

  it('skips malformed entries instead of mapping a bogus amount', () => {
    const map = parseTopUpProducts('abc:prod_a,1000:,:prod_b,2500:prod_c');
    expect(map.size).toBe(1);
    expect(map.get(2500)).toBe('prod_c');
  });
});

describe('topUpProductId', () => {
  const raw = '1000:prod_a,2500:prod_b,5000:prod_c,300:prod_rogue';

  it('resolves an offered amount', () => {
    expect(topUpProductId(raw, 2500)).toBe('prod_b');
  });

  it('refuses an amount we do not offer, even if a product is configured', () => {
    // 300 is mapped in the env var but is not in TOPUP_OPTIONS_CENTS. The
    // amount arrives from the browser, so the price list is the authority.
    expect(topUpProductId(raw, 300)).toBeNull();
  });

  it('refuses an offered amount with no product configured', () => {
    expect(topUpProductId('1000:prod_a', 5000)).toBeNull();
  });
});

describe('creditForOrder', () => {
  it('credits the amount actually paid, keyed by the order id', () => {
    expect(creditForOrder(order())).toEqual({
      user_id: USER,
      delta_cents: 1000,
      reason: 'topup',
      job_id: null,
      external_id: 'polar:order:ord_123',
    });
  });

  it('credits net_amount, not the tax-inclusive total', () => {
    // Polar is the merchant of record: the buyer may pay $12.30 on a $10
    // top-up. Crediting the total would hand out the VAT as balance.
    expect(creditForOrder(order({ netAmount: 1000 }))?.delta_cents).toBe(1000);
  });

  it('falls back to the customer external id when metadata is missing', () => {
    expect(creditForOrder(order({ metadata: {} }))?.user_id).toBe(USER);
  });

  it('refuses an order that is not paid', () => {
    expect(creditForOrder(order({ paid: false }))).toBeNull();
  });

  it('refuses a currency we cannot store', () => {
    // The ledger is USD cents with no currency column; a EUR amount would be
    // silently credited as if it were dollars.
    expect(creditForOrder(order({ currency: 'eur' }))).toBeNull();
  });

  it('refuses a subscription order', () => {
    // Subscriptions are not sold yet. If one ever is, its order must not
    // quietly become wallet credit as well as plan minutes.
    expect(creditForOrder(order({ billingReason: 'subscription_cycle' }))).toBeNull();
  });

  it('refuses a zero or negative amount', () => {
    expect(creditForOrder(order({ netAmount: 0 }))).toBeNull();
    expect(creditForOrder(order({ netAmount: -500 }))).toBeNull();
  });

  it('refuses an order with no identifiable user', () => {
    expect(creditForOrder(order({ metadata: {}, customer: { externalId: null } }))).toBeNull();
  });

  it('refuses a user id that is not a uuid', () => {
    // ledger.user_id is a uuid FK; a junk value would be an insert error at
    // best, and at worst a credit attached to nobody.
    expect(creditForOrder(order({ metadata: { user_id: 'not-a-uuid' }, customer: {} }))).toBeNull();
  });
});
