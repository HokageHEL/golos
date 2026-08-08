'use client';
import { useState, type FormEvent } from 'react';
import { useTranslations } from 'next-intl';
import { browserClient } from '@/lib/supabase/browser';

export default function LoginPage() {
  const t = useTranslations('login');
  const tErrors = useTranslations('errors');
  const [email, setEmail] = useState('');
  const [sent, setSent] = useState(false);
  const [error, setError] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(false);

    const { error: signInError } = await browserClient().auth.signInWithOtp({
      email,
      options: { emailRedirectTo: `${window.location.origin}/auth/callback` },
    });

    if (signInError) {
      setError(true);
      return;
    }

    setSent(true);
  }

  return (
    <main style={{ padding: '64px 48px', maxWidth: 400 }}>
      <h1 style={{ fontSize: 32, fontWeight: 700, margin: 0 }}>{t('title')}</h1>

      {sent ? (
        <p style={{ fontSize: 16, color: '#8C897F', marginTop: 24 }}>{t('sent')}</p>
      ) : (
        <form
          onSubmit={handleSubmit}
          style={{ display: 'flex', flexDirection: 'column', gap: 12, marginTop: 24 }}
        >
          <label style={{ fontSize: 14, fontWeight: 500 }} htmlFor="email">
            {t('emailLabel')}
          </label>
          <input
            id="email"
            type="email"
            required
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            style={{
              padding: '10px 12px',
              fontSize: 16,
              border: '1px solid #ECEAE4',
              borderRadius: 6,
            }}
          />
          <button
            type="submit"
            style={{
              padding: '10px 12px',
              fontSize: 15,
              fontWeight: 600,
              color: '#FAFAF8',
              background: '#1D6F5C',
              border: 'none',
              borderRadius: 6,
              cursor: 'pointer',
            }}
          >
            {t('submit')}
          </button>
          {error && (
            <p style={{ fontSize: 14, color: '#B3261E', margin: 0 }}>{tErrors('generic')}</p>
          )}
        </form>
      )}
    </main>
  );
}
