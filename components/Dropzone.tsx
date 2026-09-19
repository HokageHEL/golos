'use client';
import { useEffect, useRef, useState, type ChangeEvent, type DragEvent } from 'react';
import Script from 'next/script';
import { useTranslations } from 'next-intl';
import { useRouter } from '@/i18n/routing';
import { isSupportedAudio, MAX_UPLOAD_BYTES } from '@/lib/jobs/media';
import { uploadToStorage } from '@/lib/upload';
import { beginUpload, reportProgress, reportFailure } from '@/lib/upload-registry';
import Toast from './Toast';

type Turnstile = {
  render(element: HTMLElement, options: Record<string, unknown>): string;
  remove(id: string): void;
  reset(id: string): void;
};
function turnstile() { return (window as typeof window & { turnstile?: Turnstile }).turnstile; }

export default function Dropzone({ signedIn = false }: { signedIn?: boolean }) {
  const t = useTranslations();
  const router = useRouter();
  const input = useRef<HTMLInputElement>(null);
  const widget = useRef<HTMLDivElement>(null);
  const widgetId = useRef<string | null>(null);
  const busy = useRef(false);
  const [pending, setPending] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const api = turnstile();
    if (signedIn || !ready || !widget.current || !api) return;
    const id = api.render(widget.current, {
      sitekey: process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY,
      size: 'flexible', appearance: 'interaction-only', callback: setToken,
      'expired-callback': () => setToken(null), 'error-callback': () => setToken(null),
    });
    widgetId.current = id;
    return () => { api.remove(id); widgetId.current = null; };
  }, [ready, signedIn]);

  async function handleFile(file: File) {
    if (busy.current) return;
    if (!isSupportedAudio(file.type)) { setToast(t('errors.unsupportedFormat')); return; }
    if (file.size === 0 || file.size > MAX_UPLOAD_BYTES) { setToast(t('errors.tooLarge')); return; }
    if (!signedIn && !token) { setToast(t('errors.botCheck')); return; }
    busy.current = true; setPending(true);
    try {
      let fingerprint = '';
      if (!signedIn) {
        const { default: FingerprintJS } = await import('@fingerprintjs/fingerprintjs');
        fingerprint = (await (await FingerprintJS.load()).get()).visitorId;
      }
      const res = await fetch('/api/jobs/init', { method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ filename: file.name, mimeType: file.type, size: file.size, fingerprint, turnstileToken: token }),
        signal: AbortSignal.timeout(30000),
      });
      const data = await res.json();
      if (!res.ok) {
        const key = data.error === 'already_used' ? 'trial.exhausted' : data.error === 'insufficient_balance' ? 'errors.insufficientBalance'
          : data.error === 'bot_check_failed' ? 'errors.botCheck' : ['trial_unavailable', 'budget_exhausted'].includes(data.error) ? 'errors.trialUnavailable' : 'errors.generic';
        setToast(t(key)); return;
      }
      const { jobId, uploadUrl } = data;
      beginUpload(jobId);
      router.push(`/j/${jobId}`);
      try { await uploadToStorage(uploadUrl, file, (fraction) => reportProgress(jobId, fraction)); }
      catch { reportFailure(jobId, 'upload'); return; }
      const started = await fetch(`/api/jobs/${jobId}/start`, { method: 'POST' }).catch(() => null);
      if (!started?.ok) reportFailure(jobId, 'start');
    } catch { setToast(t('errors.generic')); }
    finally {
      busy.current = false; setPending(false); setToken(null);
      if (widgetId.current) turnstile()?.reset(widgetId.current);
    }
  }
  function onDrop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault(); setDragging(false);
    const file = event.dataTransfer.files[0]; if (file) void handleFile(file);
  }
  function onChange(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0]; event.target.value = ''; if (file) void handleFile(file);
  }
  return <>
    {!signedIn && <Script src="https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit" onReady={() => setReady(true)} onError={() => setToast(t('errors.botCheck'))} />}
    <div className="dropzone" data-dragging={dragging} aria-busy={pending}
      onDragOver={(event) => { event.preventDefault(); setDragging(true); }} onDragLeave={() => setDragging(false)} onDrop={onDrop}>
      <p className="subtitle">{t('landing.dropzone')}</p>
      <p className="small" style={{ margin: '4px 0 20px' }}>{t('landing.formats')}</p>
      <button type="button" className="btn btn-primary" disabled={pending} onClick={() => input.current?.click()}>{t(pending ? 'job.loading' : 'landing.chooseFile')}</button>
      <input ref={input} type="file" accept="audio/*,video/mp4" hidden onChange={onChange} />
      {!signedIn && <div ref={widget} style={{ width: '100%', maxWidth: 300, marginTop: 12 }} />}
    </div>
    {toast && <Toast message={toast} onDismiss={() => setToast(null)} />}
  </>;
}
