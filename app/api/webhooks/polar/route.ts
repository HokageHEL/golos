import { NextResponse } from 'next/server';
import { validateEvent, WebhookVerificationError } from '@polar-sh/sdk/webhooks.js';
import { syncSubscription } from '@/lib/billing/subscription';
import { parseTopUpProducts } from '@/lib/billing/credit';
import { creditForOrder } from '@/lib/billing/credit';
import { serviceClient } from '@/lib/supabase/server';

/**
 * Polar delivers here when money moves. The only event that grants credit is
 * `order.paid`; everything else is acknowledged and dropped.
 *
 * Signature verification is the SDK's, not ours, for one specific reason:
 * Polar's dashboard secret is a plain string that the Standard Webhooks
 * verifier expects base64-encoded, and `validateEvent` does that conversion.
 * Hand-rolling the HMAC would have verified against the wrong key.
 */
export async function POST(request: Request) {
  const secret = process.env.POLAR_WEBHOOK_SECRET;
  if (!secret) {
    // Same rule as the cron route: an unset secret locks the endpoint rather
    // than turning every unsigned request into a valid one.
    console.error('polar webhook: POLAR_WEBHOOK_SECRET is unset');
    return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
  }

  // Must be the raw body: the signature covers the exact bytes sent.
  const body = await request.text();
  const headers: Record<string, string> = {};
  request.headers.forEach((value, key) => {
    headers[key] = value;
  });

  let event;
  try {
    event = validateEvent(body, headers, secret);
  } catch (error) {
    if (error instanceof WebhookVerificationError) {
      return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
    }
    // A signed payload we cannot parse — a new event type, most likely. It is
    // authentic, so 401 would be a lie, and retrying will not help.
    console.error('polar webhook: signed payload failed to parse', error);
    return NextResponse.json({ error: 'invalid_event' }, { status: 400 });
  }

  if (event.type.startsWith('subscription.')) {
    try {
      await syncSubscription(event.data.id);
      return NextResponse.json({ ok: true });
    } catch (error) {
      console.error('subscription sync failed', error);
      return NextResponse.json({ error: 'subscription_failed' }, { status: 503 });
    }
  }
  if (event.type !== 'order.paid') {
    return NextResponse.json({ ok: true });
  }

  const order = event.data;
  if (!order.productId || ![...parseTopUpProducts(process.env.POLAR_TOPUP_PRODUCTS).values()].includes(order.productId)) {
    return NextResponse.json({ ok: true });
  }
  const entry = creditForOrder({
    id: order.id,
    paid: order.paid,
    currency: order.currency,
    netAmount: order.netAmount,
    billingReason: order.billingReason,
    metadata: order.metadata,
    customer: { externalId: order.customer.externalId },
  });

  if (!entry) {
    // Deliberate: a paid order we decline to credit is a decision, not a
    // failure. Logged loudly because it means someone paid and got nothing.
    console.warn(`polar webhook: order ${order.id} paid but not creditable`);
    return NextResponse.json({ ok: true });
  }

  const { error } = await serviceClient().from('ledger').insert(entry);

  // 23505 is the unique constraint on external_id — this order was already
  // credited by an earlier delivery. That is success, not an error.
  if (error && error.code !== '23505') {
    // 500 makes Polar retry. Swallowing it would lose the credit silently,
    // which is the one outcome a paying customer must never get.
    console.error(`polar webhook: crediting order ${order.id} failed`, error);
    return NextResponse.json({ error: 'credit_failed' }, { status: 500 });
  }

  return NextResponse.json({ ok: true });
}
