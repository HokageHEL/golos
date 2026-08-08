import { createClient } from '@supabase/supabase-js';

/**
 * Service-role client. Bypasses RLS — never import this into a client component.
 */
export function serviceClient() {
  return createClient(
    process.env.NEXT_PUBLIC_SUPABASE_URL!,
    process.env.SUPABASE_SERVICE_ROLE_KEY!,
    { auth: { persistSession: false } },
  );
}
