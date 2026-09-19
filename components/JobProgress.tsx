'use client';

import { useTranslations } from 'next-intl';

/**
 * Upload has a real number to show. Transcription does not — no provider
 * reports partial progress — so the bar goes indeterminate rather than
 * animating a percentage nobody measured.
 */
export default function JobProgress({
  phase,
  uploadFraction,
  filename,
  durationSeconds,
}: {
  phase: 'uploading' | 'processing';
  uploadFraction: number | null;
  filename: string;
  durationSeconds: number | null;
}) {
  const t = useTranslations('job');
  const isUploading = phase === 'uploading';

  // Null after a hard refresh mid-upload: the XHR reporting the fraction
  // belonged to the page that just went away.
  const determinate = isUploading && uploadFraction !== null;

  return (
    <div className="card card--pad-lg" style={{ width: '100%', maxWidth: 560 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 24 }}>
        <div className="icon-badge" style={{ width: 42, height: 42, flexShrink: 0 }}>
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M9 18V5l12-2v13" />
            <circle cx="6" cy="18" r="3" />
            <circle cx="18" cy="16" r="3" />
          </svg>
        </div>
        <div style={{ minWidth: 0 }}>
          <p className="truncate" style={{ fontSize: '0.9375rem', fontWeight: 600 }}>
            {filename}
          </p>
          <p className="muted" style={{ fontSize: '0.875rem', marginTop: 2 }}>
            {isUploading ? t('uploading') : t('processing')}
          </p>
        </div>
      </div>

      <div className="progress-track" role="progressbar" aria-label={isUploading ? t('uploading') : t('processing')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={determinate ? Math.round(uploadFraction * 100) : undefined}>
        <div
          className="progress-bar"
          style={{
            width: determinate ? `${Math.round(uploadFraction * 100)}%` : '100%',
            opacity: determinate ? 1 : 0.35,
            animation: determinate ? undefined : 'golos-pulse 1.6s ease-in-out infinite',
          }}
        />
      </div>

      {/* Only shown once the provider has told us how long the audio is, which
          is at completion. Before that any estimate would be invented. */}
      {!isUploading && durationSeconds !== null && (
        <p className="small" style={{ marginTop: 14 }}>
          {t('etaUnknown', { minutes: Math.max(1, Math.round(durationSeconds / 60 / 20)) })}
        </p>
      )}
    </div>
  );
}
