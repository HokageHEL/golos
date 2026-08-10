import { cookies } from 'next/headers';
import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { currentUser } from '@/lib/auth';

export async function GET(_: Request, ctx: { params: Promise<{ id: string }> }) {
  const { id } = await ctx.params;
  const db = serviceClient();

  const { data: job } = await db
    .from('jobs')
    .select('id, status, filename, duration_seconds, error, user_id, anon_key')
    .eq('id', id)
    .single();

  if (!job) return NextResponse.json({ error: 'not_found' }, { status: 404 });

  const user = await currentUser();
  const anon = (await cookies()).get('golos_anon')?.value ?? null;

  // Export is gated on ownership by account, not on merely being able to read:
  // a signed-in visitor holding the anon cookie can view an anonymous job but
  // cannot export it, and the export route enforces exactly this.
  const isOwner = job.user_id !== null && job.user_id === user?.id;
  const mayRead = isOwner || (job.anon_key !== null && job.anon_key === anon);

  if (!mayRead) return NextResponse.json({ error: 'not_found' }, { status: 404 });

  const { data: transcript } =
    job.status === 'done'
      ? await db.from('transcripts').select('text, segments, word_count').eq('job_id', id).single()
      : { data: null };

  return NextResponse.json({
    id: job.id,
    status: job.status,
    filename: job.filename,
    durationSeconds: job.duration_seconds,
    error: job.error,
    canExport: isOwner,
    transcript,
  });
}
