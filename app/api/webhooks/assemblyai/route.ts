import { NextResponse } from 'next/server';
import { serviceClient } from '@/lib/supabase/server';
import { UUID } from '@/lib/api';
import { transcriptionProvider } from '@/lib/transcription';
import { applyCompletion } from '@/lib/jobs/apply-completion';

export async function POST(request: Request) {
  try {
    if (!transcriptionProvider().verifyWebhook(request.headers)) {
      return NextResponse.json({ error: 'unauthorized' }, { status: 401 });
    }
    const body = await request.json().catch(() => null);
    if (typeof body?.transcript_id !== 'string' || body.transcript_id.length > 200) {
      return NextResponse.json({ error: 'invalid_request' }, { status: 400 });
    }
    const jobId = new URL(request.url).searchParams.get('job');
    if (jobId && UUID.test(jobId)) {
      const { error } = await serviceClient().from('jobs').update({ provider_job_id: body.transcript_id })
        .eq('id', jobId).eq('status', 'processing').is('provider_job_id', null);
      if (error) throw error;
    }
    // Acknowledge only committed work. Timeouts/failures cause provider retries;
    // the cron is a second recovery path. Replays are atomic no-ops.
    await applyCompletion(body.transcript_id);
    return NextResponse.json({ ok: true });
  } catch (error) {
    console.error('transcription completion failed', error);
    return NextResponse.json({ error: 'completion_failed' }, { status: 503 });
  }
}
