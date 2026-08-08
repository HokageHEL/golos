import { createBrowserClient } from '@supabase/ssr';

/**
 * Anon-key client for use in client components. Subject to row-level security.
 */
export function browserClient() {
  return createBrowserClient(
    process.env.NEXT_PUBLIC_SUPABASE_URL!,
    process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY!,
  );
}
