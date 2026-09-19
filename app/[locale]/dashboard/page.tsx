import { getLocale, getTranslations } from 'next-intl/server';
import { currentUser, sessionClient } from '@/lib/auth';
import { Link, redirect } from '@/i18n/routing';
import { claimTrials } from '@/lib/jobs/ownership';
import DashboardRefresh from '@/components/DashboardRefresh';
import AccountActions from '@/components/AccountActions';
import BillingButton from '@/components/BillingButton';
import ExportMenu from '@/components/ExportMenu';
import { formatDate, formatMinuteCount, formatMinutes, formatMoney } from '@/lib/format';

type JobRow = {
  id: string;
  filename: string;
  status: 'awaiting_upload' | 'processing' | 'done' | 'failed';
  duration_seconds: number | null;
  cost_cents: number | null;
  created_at: string;
};

const STATUS_KEY = {
  awaiting_upload: 'statusAwaitingUpload',
  processing: 'statusProcessing',
  done: 'statusDone',
  failed: 'statusFailed',
} as const;

export default async function DashboardPage({
  searchParams,
}: {
  searchParams: Promise<{ topup?: string; page?: string }>;
}) {
  const locale = await getLocale();
  const user = await currentUser();
  const { topup, page } = await searchParams;

  // The auth callback lands here, so an unfinished sign-in must not dead-end.
  if (!user) redirect({ href: '/login', locale });

  await claimTrials(user!.id);
  const pageNumber = Math.max(1, Math.min(10000, Math.floor(Number(page)) || 1));
  const t = await getTranslations('dashboard');
  const supabase = await sessionClient();

  // Row-level security scopes both of these to the signed-in user, so neither
  // query needs its own ownership filter.
  const [{ data: account, error: accountError }, { data: jobs, error: jobsError }] =
    await Promise.all([
      supabase
        .from('accounts')
        .select('balance_cents, minutes_included, minutes_used, subscription_id')
        .single(),
      supabase
        .from('jobs')
        .select('id, filename, status, duration_seconds, cost_cents, created_at')
        .order('created_at', { ascending: false })
        .range((pageNumber - 1) * 25, pageNumber * 25).returns<JobRow[]>(),
    ]);

  // Swallowing these renders a confident $0.00 and an empty history over what
  // is really a failed read — the worst possible lie on an account page.
  if (accountError) throw accountError;
  if (jobsError) throw jobsError;

  const minutesUsed = account?.minutes_used ?? 0;
  const minutesIncluded = account?.minutes_included ?? 0;
  const hasMore = (jobs?.length ?? 0) > 25;
  const history = (jobs ?? []).slice(0, 25);

  return (
    <main className="page">
      <DashboardRefresh enabled={topup === 'success' || history.some((job) => ['processing', 'awaiting_upload'].includes(job.status))} />
      <header className="stack rise" style={{ marginBottom: 40 }}>
        <h1 className="display">{t('heading')}</h1>
        <div className="toolbar"><AccountActions />{account?.subscription_id && <BillingButton signedIn portal />}</div>
      </header>

      {topup === 'success' && (
        // Polar redirects here the moment the buyer pays, but the credit lands
        // via webhook a beat later. Promising the balance is already updated
        // would make a correct system look broken on a slow delivery.
        <p role="status" className="notice" style={{ marginBottom: 28 }}>
          {t('topUpSuccess')}
        </p>
      )}

      <div className="duo">
        <section className="card">
          <h2 className="label">{t('balance')}</h2>
          <p className="metric" style={{ marginTop: 10 }}>
            {formatMoney(account?.balance_cents ?? 0, locale)}
          </p>
          <Link href="/pricing" className="small" style={{ marginTop: 12, display: 'inline-block' }}>
            {t('addCredit')}
          </Link>
        </section>

        <section className="card">
          <h2 className="label">{t('usedThisMonth')}</h2>
          <p className="metric" style={{ marginTop: 10 }}>
            {minutesIncluded > 0
              ? t('planUsage', {
                  used: formatMinuteCount(minutesUsed, locale),
                  total: formatMinuteCount(minutesIncluded, locale),
                })
              : formatMinuteCount(minutesUsed, locale)}
          </p>
        </section>
      </div>

      <h2 className="title" style={{ fontSize: '1.5rem', margin: '44px 0 16px' }}>
        {t('history')}
      </h2>

      <section className="card card--pad-lg">
        {history.length === 0 ? (
          <p className="muted" style={{ fontSize: '0.9375rem' }}>
            {t('empty')}
          </p>
        ) : (
          <ul className="list">
            {history.map((job) => (
              <li key={job.id} className="list-item">
                <div style={{ minWidth: 0 }}>
                  <p className="truncate" style={{ fontSize: '0.9375rem', fontWeight: 600 }}>
                    <Link href={`/j/${job.id}`}>{job.filename}</Link>
                  </p>
                  <p className="small" style={{ marginTop: 4 }}>
                    {formatDate(job.created_at, locale)}
                    {job.duration_seconds !== null &&
                      ` · ${formatMinutes(job.duration_seconds, locale)}`}
                    {job.cost_cents !== null && ` · ${formatMoney(job.cost_cents, locale)}`}
                    {job.status !== 'done' && ` · ${t(STATUS_KEY[job.status])}`}
                  </p>
                </div>

                <div className="toolbar">{job.status === 'done' && <ExportMenu jobId={job.id} canExport />}
                {['done', 'failed'].includes(job.status) && <AccountActions jobId={job.id} />}</div>
              </li>
            ))}
          </ul>
        )}
      </section>
      <div className="toolbar">
        {pageNumber > 1 && <Link href={`/dashboard?page=${pageNumber - 1}`}>{t('previous')}</Link>}
        {hasMore && <Link href={`/dashboard?page=${pageNumber + 1}`}>{t('next')}</Link>}
      </div>
    </main>
  );
}
