import { NextResponse } from 'next/server';
import { currentUser } from '@/lib/auth';
import { polarClient } from '@/lib/billing/subscription';
import { apiError, appUrl, sameOrigin } from '@/lib/api';

export async function POST(request: Request) {
  if (!sameOrigin(request)) return apiError('forbidden', 403);
  const user = await currentUser();
  if (!user) return apiError('unauthorized', 401);
  try {
    const session = await polarClient().customerSessions.create({ externalCustomerId: user.id, returnUrl: `${appUrl()}/dashboard` });
    return NextResponse.json({ url: session.customerPortalUrl });
  } catch (error) {
    console.error('billing portal failed', error);
    return apiError('checkout_unavailable', 503);
  }
}
