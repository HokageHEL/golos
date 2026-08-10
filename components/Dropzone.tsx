'use client';

import { useEffect, useRef, useState, type ChangeEvent, type DragEvent } from 'react';
import { useTranslations } from 'next-intl';
import FingerprintJS from '@fingerprintjs/fingerprintjs';
import { useRouter } from '@/i18n/routing';
import { isSupportedAudio } from '@/lib/jobs/media';
import { uploadToStorage } from '@/lib/upload';
import { beginUpload, reportProgress, reportFailure } from '@/lib/upload-registry';
import Toast from '@/components/Toast';

/**
 * The API can fail for reasons the browser can't predict (trial already
 * claimed, daily anon budget spent, bot check failed, ...). Known codes get
 * a specific, translated message; anything else falls back to a generic one.
 */
function messageForError(t: ReturnType<typeof useTranslations>, code: string | undefined): string {
  switch (code) {
    case 'unsupported_format':
      return t('errors.unsupportedFormat');
    case 'insufficient_balance':
      return t('errors.insufficientBalance');
    case 'already_used':
      return t('trial.exhausted');
    default:
      return t('errors.generic');
  }
}

export default function Dropzone() {
  const t = useTranslations();
  const router = useRouter();
  const inputRef = useRef<HTMLInputElement>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [turnstileToken, setTurnstileToken] = useState<string | null>(null);

  // Cloudflare's invisible widget calls this global by name once it has a token.
  useEffect(() => {
    const win = window as typeof window & { onTurnstileToken?: (token: string) => void };
    win.onTurnstileToken = (token: string) => setTurnstileToken(token);
    return () => {
      delete win.onTurnstileToken;
    };
  }, []);

  async function handleFile(file: File) {
    if (!isSupportedAudio(file.type)) {
      setToast(t('errors.unsupportedFormat'));
      return;
    }

    const fp = await (await FingerprintJS.load()).get();
    const init = await fetch('/api/jobs/init', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        filename: file.name,
        mimeType: file.type,
        fingerprint: fp.visitorId,
        turnstileToken,
      }),
    });

    if (!init.ok) {
      const body = (await init.json()) as { error?: string };
      setToast(messageForError(t, body.error));
      return;
    }

    const { jobId, uploadUrl } = (await init.json()) as { jobId: string; uploadUrl: string };

    // Register before navigating: this component unmounts on push, so the
    // job page must be able to find the upload the moment it mounts.
    beginUpload(jobId);

    // Navigate before the upload finishes: the user gets a shareable URL
    // immediately, and refreshing mid-upload doesn't lose the job.
    router.push(`/j/${jobId}?uploading=1`);

    // Past this point the user is looking at the job page, not this toast —
    // failures have to be reported there or they surface nowhere at all.
    try {
      await uploadToStorage(uploadUrl, file, (fraction) => reportProgress(jobId, fraction));
    } catch {
      reportFailure(jobId, 'upload');
      return;
    }

    const started = await fetch(`/api/jobs/${jobId}/start`, { method: 'POST' }).catch(() => null);
    if (!started?.ok) reportFailure(jobId, 'start');
  }

  function onDrop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault();
    setIsDragging(false);
    const file = event.dataTransfer.files[0];
    if (file) void handleFile(file);
  }

  function onInputChange(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (file) void handleFile(file);
  }

  return (
    <>
      <div
        onClick={() => inputRef.current?.click()}
        onDragOver={(event) => {
          event.preventDefault();
          setIsDragging(true);
        }}
        onDragLeave={() => setIsDragging(false)}
        onDrop={onDrop}
        style={{
          width: '100%',
          maxWidth: 560,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          padding: '56px 32px',
          borderRadius: 20,
          border: `2px dashed ${isDragging ? '#1D6F5C' : '#D8D5CD'}`,
          background: isDragging ? '#1D6F5C0A' : '#fff',
          cursor: 'pointer',
          transition: 'border-color 0.2s ease, background 0.2s ease',
        }}
      >
        <div
          style={{
            width: 56,
            height: 56,
            borderRadius: '50%',
            background: '#F1EFE9',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            marginBottom: 20,
          }}
        >
          <svg
            width="22"
            height="22"
            viewBox="0 0 24 24"
            fill="none"
            stroke="#1D6F5C"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="17 8 12 3 7 8" />
            <line x1="12" y1="3" x2="12" y2="15" />
          </svg>
        </div>
        <p style={{ fontSize: 17, fontWeight: 600, color: '#1C1B19', margin: 0 }}>
          {t('landing.dropzone')}
        </p>
        <p style={{ fontSize: 14, color: '#8C897F', margin: '4px 0 20px' }}>
          {t('landing.formats')}
        </p>
        <button
          type="button"
          style={{
            padding: '11px 22px',
            borderRadius: 10,
            border: 'none',
            background: '#1C1B19',
            color: '#fff',
            fontSize: 15,
            fontWeight: 600,
            cursor: 'pointer',
          }}
        >
          {t('landing.chooseFile')}
        </button>
        <input
          ref={inputRef}
          type="file"
          accept="audio/*,video/mp4"
          hidden
          onChange={onInputChange}
        />
        <div
          className="cf-turnstile"
          data-sitekey={process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY}
          data-callback="onTurnstileToken"
          data-size="invisible"
        />
      </div>
      {toast && <Toast message={toast} onDismiss={() => setToast(null)} />}
    </>
  );
}
