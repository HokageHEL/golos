'use client';
import { useState } from 'react';
import { useLocale, useTranslations } from 'next-intl';
import { useRouter } from '@/i18n/routing';
import Toast from './Toast';

export default function BillingButton({ signedIn, portal = false }: { signedIn: boolean; portal?: boolean }) {
  const t = useTranslations('pricing');
  const locale = useLocale();
  const router = useRouter();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState(false);
  async function open() {
    if (!signedIn) { router.push('/login'); return; }
    setPending(true);
    try {
      const res = await fetch(portal ? '/api/billing/portal' : '/api/checkout', {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ subscription: true, locale }),
      });
      const data = await res.json();
      if (!res.ok || !data.url) throw new Error('Checkout failed');
      window.location.assign(data.url);
    } catch { setError(true); setPending(false); }
  }
  return <>
    <button className="btn" disabled={pending} onClick={open}>{pending ? t('redirecting') : t(portal ? 'manage' : 'subscribe')}</button>
    {error && <Toast message={t('checkoutFailed')} onDismiss={() => setError(false)} />}
  </>;
}
