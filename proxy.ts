import createMiddleware from 'next-intl/middleware';
import { createServerClient } from '@supabase/ssr';
import type { NextRequest } from 'next/server';
import { routing } from './i18n/routing';

const localize = createMiddleware(routing);
export default async function proxy(request: NextRequest) {
  const response = localize(request);
  if (!request.cookies.getAll().some(({ name }) => name.startsWith('sb-'))) return response;
  const supabase = createServerClient(
    process.env.NEXT_PUBLIC_SUPABASE_URL!, process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY!, {
      cookies: {
        getAll: () => request.cookies.getAll(),
        setAll: (values) => {
          for (const { name, value, options } of values) {
            request.cookies.set(name, value);
            response.cookies.set(name, value, options);
          }
          // Forward refreshed cookies to the same request's Server Components.
          response.headers.set('x-middleware-request-cookie', request.headers.get('cookie') || '');
          const overrides = new Set((response.headers.get('x-middleware-override-headers') || '').split(',').filter(Boolean));
          overrides.add('cookie');
          response.headers.set('x-middleware-override-headers', [...overrides].join(','));
        },
      },
    },
  );
  await supabase.auth.getClaims();
  response.headers.set('Cache-Control', 'private, no-store');
  return response;
}
export const config = { matcher: ['/((?!api/|auth/|_next/|_vercel/|.*\\..*).*)'] };
