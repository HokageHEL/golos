'use client';
import { Suspense, useState, type FormEvent } from 'react';
import { useSearchParams } from 'next/navigation';
import { useLocale, useTranslations } from 'next-intl';
import { browserClient } from '@/lib/supabase/browser';

export default function LoginPage() {
  // useSearchParams opts the tree into client rendering; the boundary keeps
  // that from opting the whole route out of static generation.
  return (
    <Suspense>
      <LoginCard />
    </Suspense>
  );
}

function LoginCard() {
  const t = useTranslations('login');
  const locale = useLocale();
  const [pending, setPending] = useState(false);
  const tErrors = useTranslations('errors');
  const [email, setEmail] = useState('');
  const [sent, setSent] = useState(false);
  const [error, setError] = useState(false);
  // Set by /auth/callback when a magic link has already been used or expired.
  const expired = useSearchParams().get('error') === 'link_expired';

  async function handleGoogle() {
    setError(false);
    setPending(true);
    try {

    const { error: oauthError } = await browserClient().auth.signInWithOAuth({
      provider: 'google',
      options: { redirectTo: `${window.location.origin}/auth/callback?locale=${locale}` },
    });

    if (oauthError) setError(true);
    } catch { setError(true); } finally { setPending(false); }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(false);
    setPending(true);
    try {

    const { error: signInError } = await browserClient().auth.signInWithOtp({
      email,
      options: { emailRedirectTo: `${window.location.origin}/auth/callback?locale=${locale}` },
    });

    if (signInError) {
      setError(true);
      return;
    }

    setSent(true);
    } catch { setError(true); } finally { setPending(false); }
  }

  return (
    <main className="page page--narrow page--tight">
      <div className="card card--pad-lg rise" style={{ boxShadow: 'var(--shadow-md)' }}>
        <h1 className="title" style={{ fontSize: '1.75rem', textAlign: 'center' }}>
          {t('title')}
        </h1>

        {expired && !sent && (
          <p
            role="status"
            style={{ marginTop: 16, fontSize: '0.875rem', color: 'var(--danger)' }}
          >
            {t('linkExpired')}
          </p>
        )}

        {sent ? (
          <p className="muted" style={{ marginTop: 16, textAlign: 'center' }}>
            {t('sent')}
          </p>
        ) : (
          <>
            {process.env.NEXT_PUBLIC_GOOGLE_AUTH_ENABLED === 'true' && <><button
              disabled={pending}
              type="button"
              onClick={handleGoogle}
              className="btn btn-secondary btn-block"
              style={{ marginTop: 24 }}
            >
              {t('google')}
            </button>

            <div className="divider" style={{ margin: '20px 0' }}>
              {t('or')}
            </div>

            </>}
            <form
              onSubmit={handleSubmit}
              style={{ display: 'flex', flexDirection: 'column', gap: 12 }}
            >
              <div className="field">
                <label className="label" htmlFor="email">
                  {t('emailLabel')}
                </label>
                <input
                  id="email"
                  className="input"
                  type="email"
                  required
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                />
              </div>
              <button disabled={pending} type="submit" className="btn btn-accent btn-block">
                {t(pending ? 'sending' : 'submit')}
              </button>
              {error && (
                <p role="alert" style={{ fontSize: '0.875rem', color: 'var(--danger)' }}>{tErrors('generic')}</p>
              )}
            </form>
          </>
        )}
      </div>
    </main>
  );
}
