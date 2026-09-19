import { cookies } from 'next/headers';
import { currentUser } from '@/lib/auth';
import { serviceClient } from '@/lib/supabase/server';

export async function ownedJob(id: string) {
  const db = serviceClient();
  const { data: job, error } = await db.from('jobs').select('*').eq('id', id).maybeSingle();
  if (error) throw error;
  if (!job) return null;
  const user = await currentUser();
  const anon = (await cookies()).get('golos_anon')?.value;
  if (job.user_id) return job.user_id === user?.id ? { job, user } : null;
  if (!anon || job.anon_key !== anon) return null;
  return { job, user };
}

export async function claimTrials(userId: string) {
  const anon = (await cookies()).get('golos_anon')?.value;
  if (!anon) return;
  const { error } = await serviceClient().from('jobs')
    .update({ user_id: userId, anon_key: null })
    .eq('anon_key', anon).is('user_id', null).in('status', ['done', 'failed']);
  if (error) throw error;
}
