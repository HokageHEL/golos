import { NextResponse } from 'next/server';
import { polarClient } from '@/lib/billing/subscription';
import { serviceClient } from '@/lib/supabase/server';
import { apiError, appUrl, sameOrigin } from '@/lib/api';
import { topUpProductId } from '@/lib/billing/credit';
import { currentUser } from '@/lib/auth';
import { routing } from '@/i18n/routing';

/**
 * Starts a Polar checkout for a credit top-up and hands the browser the URL to
 * send the buyer to. Server-side rather than a client-built link so the amount
 * is validated against the price list and the buyer's identity is ours to set,
 * not the browser's to claim.
 */
export async function POST(request: Request) {
  if (!sameOrigin(request)) return apiError('forbidden', 403);
  const user = await currentUser();
  if (!user) {
    return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
  }

  const accessToken = process.env.POLAR_ACCESS_TOKEN;
  if (!accessToken) {
    console.error('checkout: POLAR_ACCESS_TOKEN is unset');
    return NextResponse.json({ error: 'checkout_unavailable' }, { status: 503 });
  }

  const body = (await request.json().catch(() => null)) as {
    cents?: unknown;
    subscription?: unknown;
    locale?: unknown;
  } | null;

  const cents = typeof body?.cents === 'number' ? body.cents : NaN;
  const isSubscription = body?.subscription === true;
  const productId = isSubscription ? process.env.POLAR_SUBSCRIPTION_PRODUCT_ID : topUpProductId(process.env.POLAR_TOPUP_PRODUCTS, cents);
  if (!productId) {
    // Either the amount is not one we sell, or its product is not configured.
    // Both are the same thing to the caller: this is not purchasable.
    return NextResponse.json({ error: 'unavailable_amount' }, { status: 400 });
  }

  const locale =
    typeof body?.locale === 'string' &&
    (routing.locales as readonly string[]).includes(body.locale)
      ? body.locale
      : routing.defaultLocale;
  const prefix = locale === routing.defaultLocale ? '' : `/${locale}`;


  try {
    const polar = polarClient();
    if (isSubscription) {
      const { data: account, error } = await serviceClient().from('accounts')
        .select('subscription_id, subscription_status').eq('user_id', user.id).single();
      if (error) throw error;
      if (account?.subscription_id && ['active', 'trialing', 'past_due'].includes(account.subscription_status)) {
        const session = await polar.customerSessions.create({ externalCustomerId: user.id, returnUrl: `${appUrl()}${prefix}/dashboard` });
        return NextResponse.json({ url: session.customerPortalUrl });
      }
    }
    const checkout = await polar.checkouts.create({
      products: [productId],
      allowDiscountCodes: false,
      successUrl: `${appUrl()}${prefix}/dashboard?topup=success`,
      // Both of these carry the identity to the webhook. metadata is read
      // first there; externalCustomerId is the fallback and also links the
      // buyer to a stable Polar customer across purchases.
      externalCustomerId: user.id,
      customerEmail: user.email,
      metadata: { user_id: user.id },
    });

    return NextResponse.json({ url: checkout.url });
  } catch (error) {
    console.error('checkout: creating the Polar session failed', error);
    return NextResponse.json({ error: 'checkout_failed' }, { status: 502 });
  }
}
