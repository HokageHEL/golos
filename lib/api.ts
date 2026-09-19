import { NextResponse } from 'next/server';

export const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Browsers must originate state-changing requests on this site. */
export function sameOrigin(request: Request): boolean {
  const origin = request.headers.get('origin');
  return !origin || origin === new URL(request.url).origin || origin === process.env.NEXT_PUBLIC_APP_URL;
}

export function apiError(error: string, status: number) {
  return NextResponse.json({ error }, { status });
}

export function appUrl(): string {
  const url = new URL(process.env.NEXT_PUBLIC_APP_URL || 'http://localhost:3000');
  if (process.env.NODE_ENV === 'production' && url.protocol !== 'https:' && !['localhost', '127.0.0.1'].includes(url.hostname)) {
    throw new Error('NEXT_PUBLIC_APP_URL must use HTTPS');
  }
  return url.origin;
}
