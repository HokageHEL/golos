import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { ownedJob } from '@/lib/jobs/ownership';
import { UUID, apiError, sameOrigin } from '@/lib/api';
import { wordCount } from '@/lib/transcript/turns';
import { validateEdits } from '@/lib/transcript/edit';

export async function GET(_: Request, ctx: { params: Promise<{ id: string }> }) {
  const { id } = await ctx.params;
  if (!UUID.test(id)) return apiError('not_found', 404);
  try {
    const owner = await ownedJob(id);
    if (!owner) return apiError('not_found', 404);
    const { job, user } = owner;
    const db = serviceClient();
    const { data: transcript, error } = job.status === 'done'
      ? await db.from('transcripts').select('text, segments, word_count, updated_at').eq('job_id', id).single()
      : { data: null, error: null };
    if (error) throw error;
    return NextResponse.json({ id, status: job.status, filename: job.filename,
      durationSeconds: job.duration_seconds, error: job.error,
      truncated: job.truncated, maxDurationSeconds: job.max_duration_seconds,
      canExport: job.user_id === user?.id, transcript,
    }, { headers: { 'Cache-Control': 'private, no-store' } });
  } catch (error) {
    console.error('reading job failed', error);
    return apiError('unavailable', 503);
  }
}

export async function PATCH(request: Request, ctx: { params: Promise<{ id: string }> }) {
  if (!sameOrigin(request)) return apiError('forbidden', 403);
  const { id } = await ctx.params;
  if (!UUID.test(id)) return apiError('not_found', 404);
  try {
    const owner = await ownedJob(id);
    if (!owner || !owner.user || owner.job.user_id !== owner.user.id) return apiError('not_found', 404);
    if (owner.job.status !== 'done') return apiError('not_ready', 409);
    const body = await request.json().catch(() => null);
    const segments = validateEdits(body?.segments);
    if (!segments || typeof body?.updatedAt !== 'string') return apiError('invalid_request', 400);
    const text = segments.map((s) => s.text).join(' ');
    const { data, error } = await serviceClient().from('transcripts').update({
      segments, text, word_count: wordCount(text), updated_at: new Date().toISOString(),
    }).eq('job_id', id).eq('updated_at', body.updatedAt).select('updated_at').maybeSingle();
    if (error) throw error;
    if (!data) return apiError('edit_conflict', 409);
    return NextResponse.json({ updatedAt: data.updated_at });
  } catch (error) {
    console.error('saving transcript failed', error);
    return apiError('unavailable', 503);
  }
}

export async function DELETE(request: Request, ctx: { params: Promise<{ id: string }> }) {
  if (!sameOrigin(request)) return apiError('forbidden', 403);
  const { id } = await ctx.params;
  if (!UUID.test(id)) return apiError('not_found', 404);
  try {
    const owner = await ownedJob(id);
    if (!owner) return apiError('not_found', 404);
    if (owner.job.status === 'processing' || owner.job.status === 'awaiting_upload') return apiError('job_in_progress', 409);
    const db = serviceClient();
    const { error: mediaError } = await db.storage.from('audio').remove([owner.job.storage_path]);
    if (mediaError) throw mediaError;
    const { error } = await db.from('jobs').delete().eq('id', id);
    if (error) throw error;
    return NextResponse.json({ ok: true });
  } catch (error) {
    console.error('deleting job failed', error);
    return apiError('unavailable', 503);
  }
}
