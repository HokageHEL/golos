import { Polar } from '@polar-sh/sdk';
import { serviceClient } from '@/lib/supabase/server';
import { SUBSCRIPTION } from './pricing';
import { UUID } from '@/lib/api';

export function polarClient() {
  if (!process.env.POLAR_ACCESS_TOKEN) throw new Error('POLAR_ACCESS_TOKEN is required');
  return new Polar({ accessToken: process.env.POLAR_ACCESS_TOKEN,
    server: process.env.POLAR_SERVER === 'sandbox' ? 'sandbox' : 'production' });
}

/** Fetch current state, so delayed/reordered webhooks cannot restore an old plan. */
export async function syncSubscription(id: string) {
  const subscription = await polarClient().subscriptions.get({ id });
  if (!process.env.POLAR_SUBSCRIPTION_PRODUCT_ID || subscription.productId !== process.env.POLAR_SUBSCRIPTION_PRODUCT_ID) return;
  const userId = subscription.customer.externalId;
  if (!userId || !UUID.test(userId)) throw new Error('Subscription has no account');
  const { error } = await serviceClient().rpc('sync_subscription', {
    p_user_id: userId, p_id: subscription.id, p_status: subscription.status,
    p_start: subscription.currentPeriodStart.toISOString(), p_end: subscription.currentPeriodEnd.toISOString(),
    p_updated: (subscription.modifiedAt ?? subscription.createdAt).toISOString(), p_minutes: SUBSCRIPTION.minutesIncluded,
  });
  if (error) throw error;
}
