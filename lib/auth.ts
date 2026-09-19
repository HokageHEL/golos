import { cookies } from 'next/headers';
import { createServerClient } from '@supabase/ssr';

/**
 * Cookie-backed Supabase client for use in server components, route handlers,
 * and server actions. Subject to row-level security.
 */
export async function sessionClient() {
  const store = await cookies();

  return createServerClient(
    process.env.NEXT_PUBLIC_SUPABASE_URL!,
    process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY!,
    {
      cookies: {
        getAll: () => store.getAll(),
        setAll: (list) => {
          try {
            for (const { name, value, options } of list) store.set(name, value, options);
          } catch {
            // Server Components cannot write cookies; proxy refreshes them.
          }
        },
      },
    },
  );
}

/**
 * The single accessor the rest of the app uses to find out who is signed in.
 * Returns null when signed out.
 */
export async function currentUser() {
  const supabase = await sessionClient();
  const { data } = await supabase.auth.getUser();
  return data.user;
}
