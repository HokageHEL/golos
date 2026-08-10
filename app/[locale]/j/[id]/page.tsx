'use client';

import { useCallback, useEffect, useState, useSyncExternalStore } from 'react';
import { useParams } from 'next/navigation';
import { useTranslations } from 'next-intl';
import { Link } from '@/i18n/routing';
import { subscribeToUpload, uploadState } from '@/lib/upload-registry';
import JobProgress from '@/components/JobProgress';
import TranscriptView from '@/components/TranscriptView';
import ExportMenu from '@/components/ExportMenu';
import type { Segment } from '@/lib/transcript/types';

const POLL_INTERVAL_MS = 3000;

type Job = {
  id: string;
  status: 'awaiting_upload' | 'processing' | 'done' | 'failed';
  filename: string;
  durationSeconds: number | null;
  error: string | null;
  canExport: boolean;
  transcript: { text: string; segments: Segment[]; word_count: number } | null;
};

function formatDuration(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;
}

export default function JobPage() {
  const { id } = useParams<{ id: string }>();
  const t = useTranslations();
  const [job, setJob] = useState<Job | null>(null);
  const [missing, setMissing] = useState(false);

  const upload = useSyncExternalStore(
    useCallback((listener: () => void) => subscribeToUpload(id, listener), [id]),
    useCallback(() => uploadState(id), [id]),
    () => undefined,
  );

  const status = job?.status;
  // Nothing will change on its own once the job has settled, and a failed
  // upload means the job is stuck at awaiting_upload for good.
  const settled = status === 'done' || status === 'failed' || upload?.failure != null;

  useEffect(() => {
    if (settled) return;
    let cancelled = false;

    async function load() {
      const res = await fetch(`/api/jobs/${id}`).catch(() => null);
      if (cancelled || !res) return;
      if (res.status === 404) {
        setMissing(true);
        return;
      }
      if (res.ok) setJob((await res.json()) as Job);
    }

    void load();
    const timer = setInterval(() => void load(), POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [id, settled]);

  return (
    <main
      style={{
        padding: '48px 48px 96px',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
      }}
    >
      <div style={{ width: '100%', maxWidth: 760 }}>
        {missing ? (
          <Notice message={t('job.notFound')} />
        ) : job === null ? null : upload?.failure != null ? (
          <Notice
            message={
              upload.failure === 'upload' ? t('errors.uploadFailed') : t('errors.generic')
            }
            action={t('job.retry')}
          />
        ) : job.status === 'failed' ? (
          <Notice message={t('job.failed')} detail={job.error} action={t('job.retry')} />
        ) : job.status === 'done' && job.transcript !== null ? (
          <>
            <header
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: 24,
                marginBottom: 24,
              }}
            >
              <div style={{ minWidth: 0 }}>
                <h1 style={{ fontSize: 28, fontWeight: 700, margin: 0 }}>{t('result.heading')}</h1>
                <p style={{ fontSize: 14, color: '#8C897F', margin: '6px 0 0' }}>
                  {t('result.stats', {
                    duration: formatDuration(job.durationSeconds ?? 0),
                    words: job.transcript.word_count,
                  })}
                </p>
              </div>
              <ExportMenu jobId={job.id} canExport={job.canExport} />
            </header>
            <TranscriptView segments={job.transcript.segments} />
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
    <div
      style={{
        background: '#fff',
        border: '1px solid #ECEAE4',
        borderRadius: 16,
        padding: 40,
        textAlign: 'center',
      }}
    >
      <p style={{ fontSize: 17, fontWeight: 600, margin: 0 }}>{message}</p>
      {detail && (
        <p style={{ fontSize: 13, color: '#8C897F', margin: '8px 0 0', fontFamily: 'monospace' }}>
          {detail}
        </p>
      )}
      {action && (
        <Link
          href="/"
          style={{
            display: 'inline-block',
            marginTop: 24,
            padding: '11px 22px',
            borderRadius: 10,
            background: '#1C1B19',
            color: '#fff',
            fontSize: 15,
            fontWeight: 600,
          }}
        >
          {action}
        </Link>
      )}
    </div>
  );
}
