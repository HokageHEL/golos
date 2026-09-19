import { getLocale, getTranslations } from 'next-intl/server';
import BillingButton from '@/components/BillingButton';
import TopUpOptions from '@/components/TopUpOptions';
import { currentUser } from '@/lib/auth';
import { formatMinuteCount, formatMoney } from '@/lib/format';
import { topUpProductId } from '@/lib/billing/credit';
import { PAYG_RATE_CENTS, SUBSCRIPTION, TOPUP_OPTIONS_CENTS } from '@/lib/billing/pricing';

export default async function PricingPage() {
  const t = await getTranslations('pricing');
  const locale = await getLocale();
  const user = await currentUser();

  // Only offer amounts that actually have a Polar product behind them. An
  // unconfigured tier would render a button whose only outcome is an error.
  const buyable = TOPUP_OPTIONS_CENTS.filter(
    (cents) => !!process.env.POLAR_ACCESS_TOKEN && topUpProductId(process.env.POLAR_TOPUP_PRODUCTS, cents) !== null,
  );

  return (
    <main className="page">
      <header className="stack rise" style={{ gap: 14, marginBottom: 44 }}>
        <h1 className="display">{t('heading')}</h1>
        <p className="lede">{t('subheading')}</p>
      </header>

      <div className="duo">
        <section className="card card--pad-lg card--fill card--accent">
          <h2 className="label">{t('subscription')}</h2>
          <p className="metric" style={{ marginTop: 12 }}>
            {formatMoney(SUBSCRIPTION.priceCents, locale)}
            <span>{t('perMonth')}</span>
          </p>
          <p className="small" style={{ marginTop: 12 }}>
            {t('included', { minutes: formatMinuteCount(SUBSCRIPTION.minutesIncluded, locale) })}
          </p>
          {process.env.POLAR_ACCESS_TOKEN && process.env.POLAR_SUBSCRIPTION_PRODUCT_ID
            ? <BillingButton signedIn={user !== null} />
            : <p className="notice">{t('comingSoon')}</p>}
        </section>

        <section className="card card--pad-lg card--fill">
          <h2 className="label">{t('payg')}</h2>
          <p className="metric" style={{ marginTop: 12 }}>
            {formatMoney(PAYG_RATE_CENTS, locale)}
            <span>{t('perMinute')}</span>
          </p>
          <p className="small" style={{ marginTop: 12 }}>{t('paygDetail')}</p>

          {buyable.length > 0 ? (
            <TopUpOptions options={buyable} signedIn={user !== null} />
          ) : (
            <button
              type="button"
              disabled
              title={t('comingSoon')}
              className="btn btn-block"
              style={{ marginTop: 20 }}
            >
              {t('topUp')}
            </button>
          )}
        </section>
      </div>
    </main>
  );
}
