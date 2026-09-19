'use client';
import { useState } from 'react';
import { useTranslations } from 'next-intl';
import { useRouter } from '@/i18n/routing';
import { browserClient } from '@/lib/supabase/browser';
import Toast from './Toast';

export default function AccountActions({ jobId }: { jobId?: string }) {
  const t = useTranslations('dashboard');
  const [pending, setPending] = useState(false);
  const [error, setError] = useState(false);
  const router = useRouter();
  async function act() {
    if (jobId && !window.confirm(t('confirmDelete'))) return;
    setPending(true);
    try {
      if (jobId) {
        const res = await fetch(`/api/jobs/${jobId}`, { method: 'DELETE' });
        if (!res.ok) throw new Error('Delete failed');
      } else {
        const { error } = await browserClient().auth.signOut();
        if (error) throw error;
        router.replace('/');
      }
      router.refresh();
    } catch { setError(true); } finally { setPending(false); }
  }
  return <>
    <button className="btn btn-secondary btn-sm" disabled={pending} onClick={act}>{t(jobId ? 'delete' : 'signOut')}</button>
    {error && <Toast message={t('actionFailed')} onDismiss={() => setError(false)} />}
  </>;
}
