'use client';
import { useState } from 'react';
import { useLocale, useTranslations } from 'next-intl';
import { Link } from '@/i18n/routing';
import Toast from './Toast';
const FORMATS = ['txt', 'docx', 'srt'] as const;

export default function ExportMenu({ jobId, canExport }: { jobId: string; canExport: boolean }) {
  const t = useTranslations();
  const locale = useLocale();
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  async function download(format: string) {
    setOpen(false); setPending(true);
    try {
      const res = await fetch(`/api/jobs/${jobId}/export?format=${format}&locale=${locale}`);
      if (!res.ok) throw new Error('Export failed');
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      const encoded = /filename\*=UTF-8''([^;]+)/i.exec(res.headers.get('content-disposition') || '')?.[1];
      link.href = url; link.download = encoded ? decodeURIComponent(encoded) : `transcript.${format}`;
      document.body.appendChild(link); link.click(); link.remove();
      setTimeout(() => URL.revokeObjectURL(url), 10000);
      setToast(t('result.downloaded', { format }));
    } catch { setToast(t('errors.generic')); } finally { setPending(false); }
  }
  if (!canExport) return <div className="toolbar"><span className="small">{t('trial.signInToExport')}</span>
    <Link href="/login" className="btn btn-primary btn-sm">{t('login.title')}</Link></div>;
  return <div style={{ position: 'relative' }} onKeyDown={(e) => { if (e.key === 'Escape') setOpen(false); }}>
    <button type="button" className="btn btn-secondary btn-sm" disabled={pending} aria-expanded={open} onClick={() => setOpen(!open)}>
      {pending ? t('job.loading') : `${t('result.export')} ▾`}
    </button>
    {open && <>
      <div onClick={() => setOpen(false)} style={{ position: 'fixed', inset: 0, zIndex: 20 }} />
      <div className="menu">{FORMATS.map((format) => <button className="menu-option" type="button" key={format} onClick={() => download(format)}>.{format}</button>)}</div>
    </>}
    {toast && <Toast message={toast} onDismiss={() => setToast(null)} />}
  </div>;
}
