'use client';

import { useCallback, useEffect, useState, useSyncExternalStore } from 'react';
import { useParams } from 'next/navigation';
import { useTranslations } from 'next-intl';
import { Link } from '@/i18n/routing';
import { subscribeToUpload, uploadState, reportProgress } from '@/lib/upload-registry';
import JobProgress from '@/components/JobProgress';
import TranscriptEditor from '@/components/TranscriptEditor';

import { wordCount } from '@/lib/transcript/turns';
import type { Segment } from '@/lib/transcript/types';

const POLL_INTERVAL_MS = 3000;

type Job = {
  id: string;
  status: 'awaiting_upload' | 'processing' | 'done' | 'failed';
  filename: string;
  durationSeconds: number | null;
  error: string | null;
  canExport: boolean;
  truncated: boolean;
  maxDurationSeconds: number;
  transcript: { text: string; segments: Segment[]; word_count: number; updated_at: string } | null;
};

function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;
}

export default function JobPage() {
  const { id } = useParams<{ id: string }>();
  return <JobContent key={id} id={id} />;
}

function JobContent({ id }: { id: string }) {
  const t = useTranslations();
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState(false);
  const [job, setJob] = useState<Job | null>(null);
  const [networkError, setNetworkError] = useState(false);
  const [missing, setMissing] = useState(false);

  const upload = useSyncExternalStore(
    useCallback((listener: () => void) => subscribeToUpload(id, listener), [id]),
    useCallback(() => uploadState(id), [id]),
    () => undefined,
  );

  const status = job?.status;
  // Nothing will change on its own once the job has settled, and a failed
  // upload means the job is stuck at awaiting_upload for good.
  const settled = status === 'done' || status === 'failed' || missing;

  useEffect(() => {
    if (settled) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    async function load() {
      try {
        const res = await fetch(`/api/jobs/${id}`, { signal: AbortSignal.timeout(15000) });
        if (cancelled) return;
        if (res.status === 404) { setMissing(true); return; }
        if (!res.ok) throw new Error('Load failed');
        const data = await res.json() as Job;
        if (cancelled) return;
        setJob(data); setNetworkError(false);
      } catch { if (!cancelled) setNetworkError(true); }
      if (!cancelled) timer = setTimeout(() => void load(), POLL_INTERVAL_MS);
    }
    void load();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [id, settled]);

  async function resume() {
    setStarting(true); setStartError(false);
    try {
      const response = await fetch(`/api/jobs/${id}/start`, { method: 'POST' });
      if (!response.ok) throw new Error('Start failed');
      reportProgress(id, 1);
    } catch { setStartError(true); } finally { setStarting(false); }
  }

  return (
    <main className="page">
      <div style={{ width: '100%' }}>
        {networkError && <p role="status" className="notice">{t('job.reconnecting')}</p>}
        {job?.status === 'awaiting_upload' && (!upload || upload.failure === 'start') &&
          <div className="notice"><p>{t('job.resumeHelp')}</p><button className="btn btn-sm" disabled={starting} onClick={resume}>{t('job.resume')}</button>
          {startError && <p role="alert">{t('job.resumeFailed')}</p>}</div>}
        {missing ? (
          <Notice message={t('job.notFound')} />
        ) : job === null ? <p role="status">{t('job.loading')}</p> : upload?.failure != null && job.status === 'awaiting_upload' ? (
          <Notice
            message={
              upload.failure === 'upload' ? t('errors.uploadFailed') : t('errors.generic')
            }
            action={t('job.retry')}
          />
        ) : job.status === 'failed' ? (
          <Notice message={t('job.failed')} action={t('job.retry')} />
        ) : job.status === 'done' && job.transcript !== null ? (
          <>
            <header
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                flexWrap: 'wrap',
                gap: 16,
                marginBottom: 20,
              }}
            >
              <div style={{ minWidth: 0 }}>
                <h1 className="title" style={{ fontSize: '1.75rem' }}>
                  {t('result.heading')}
                </h1>
                <p className="small" style={{ marginTop: 6 }}>
                  {t('result.stats', {
                    duration: formatDuration(job.durationSeconds ?? 0),
                    words: job.transcript.word_count,
                  })}
                </p>
              </div>

            </header>
            {job.truncated && <p className="notice">{t('job.truncated', { minutes: Math.floor(job.maxDurationSeconds / 60) })}</p>}
            <TranscriptEditor key={job.id} jobId={job.id} segments={job.transcript.segments} canEdit={job.canExport} updatedAt={job.transcript.updated_at}
              onSaved={(segments) => setJob((current) => current?.transcript ? {
                ...current, transcript: { ...current.transcript, segments, word_count: wordCount(segments.map((s) => s.text).join(' ')) },
              } : current)} />
          </>
        ) : (
          <div style={{ display: 'flex', justifyContent: 'center' }}>
            <JobProgress
              phase={job.status === 'awaiting_upload' ? 'uploading' : 'processing'}
              uploadFraction={upload?.fraction ?? null}
              filename={job.filename}
              durationSeconds={job.durationSeconds}
            />
          </div>
        )}
      </div>
    </main>
  );
}

/**
 * `detail` is the provider's own error string — untranslated on purpose,
 * because it is a diagnostic we pass through rather than a message we wrote.
 */
function Notice({
  message,
  detail,
  action,
}: {
  message: string;
  detail?: string | null;
  action?: string;
}) {
  return (
    <div className="card card--pad-lg" style={{ textAlign: 'center' }}>
      <p className="subtitle">{message}</p>
      {detail && (
        <p className="small" style={{ marginTop: 8, fontFamily: 'ui-monospace, monospace' }}>
          {detail}
        </p>
      )}
      {action && (
        <Link href="/" className="btn btn-primary" style={{ marginTop: 24 }}>
          {action}
        </Link>
      )}
    </div>
  );
}
