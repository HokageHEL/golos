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
    <div
      style={{
        width: '100%',
        maxWidth: 560,
        background: '#fff',
        border: '1px solid #ECEAE4',
        borderRadius: 16,
        padding: 40,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 24 }}>
        <div
          style={{
            width: 40,
            height: 40,
            flexShrink: 0,
            borderRadius: 10,
            background: '#F1EFE9',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="#1D6F5C"
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
          <p
            style={{
              fontSize: 15,
              fontWeight: 600,
              margin: 0,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {filename}
          </p>
          <p style={{ fontSize: 14, color: '#8C897F', margin: '2px 0 0' }}>
            {isUploading ? t('uploading') : t('processing')}
          </p>
        </div>
      </div>

      <div style={{ width: '100%', height: 6, background: '#F1EFE9', borderRadius: 100 }}>
        <div
          style={{
            height: '100%',
            background: '#1D6F5C',
            borderRadius: 100,
            width: determinate ? `${Math.round(uploadFraction * 100)}%` : '100%',
            opacity: determinate ? 1 : 0.35,
            animation: determinate ? undefined : 'golos-pulse 1.6s ease-in-out infinite',
            transition: 'width 0.2s linear',
          }}
        />
      </div>

      {/* Only shown once the provider has told us how long the audio is, which
          is at completion. Before that any estimate would be invented. */}
      {!isUploading && durationSeconds !== null && (
        <p style={{ fontSize: 13, color: '#8C897F', margin: '14px 0 0' }}>
          {t('etaUnknown', { minutes: Math.max(1, Math.round(durationSeconds / 60 / 20)) })}
        </p>
      )}

      <style>{`
        @keyframes golos-pulse { 0%, 100% { opacity: 0.25; } 50% { opacity: 0.6; } }
      `}</style>
    </div>
  );
}
