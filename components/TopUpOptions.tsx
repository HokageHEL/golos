'use client';

import { useState } from 'react';
import { useLocale, useTranslations } from 'next-intl';
import { useRouter } from '@/i18n/routing';
import { formatMoney } from '@/lib/format';
import Toast from '@/components/Toast';

export default function TopUpOptions({
  options,
  signedIn,
}: {
  options: readonly number[];
  signedIn: boolean;
}) {
  const t = useTranslations('pricing');
  const locale = useLocale();
  const router = useRouter();
  const [selected, setSelected] = useState(options[0]);
  const [pending, setPending] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  async function startCheckout() {
    // Credit is attached to an account, so there has to be one first.
    if (!signedIn) {
      router.push('/login');
      return;
    }

    setPending(true);
    try {
      const response = await fetch('/api/checkout', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ cents: selected, locale }),
      });

      const body = (await response.json().catch(() => null)) as { url?: string } | null;

      if (!response.ok || !body?.url) {
        setToast(t('checkoutFailed'));
        setPending(false);
        return;
      }

      // Polar hosts the payment page; leave the app entirely. `pending` stays
      // true on purpose — the button must not look clickable while the
      // browser is already navigating away.
      window.location.href = body.url;
    } catch {
      setToast(t('checkoutFailed'));
      setPending(false);
    }
  }

  return (
    <div>
      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
        {options.map((cents) => (
          <button
            key={cents}
            type="button"
            className="chip"
            aria-pressed={cents === selected}
            onClick={() => setSelected(cents)}
          >
            {formatMoney(cents, locale)}
          </button>
        ))}
      </div>

      <button
        type="button"
        className="btn btn-block"
        style={{ marginTop: 20 }}
        disabled={pending}
        onClick={startCheckout}
      >
        {pending ? t('redirecting') : `${t('topUp')} · ${formatMoney(selected, locale)}`}
      </button>

      {toast && <Toast message={toast} onDismiss={() => setToast(null)} />}
    </div>
  );
}
