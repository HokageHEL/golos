import { getLocale, getTranslations } from 'next-intl/server';
import { currentUser } from '@/lib/auth';
import Dropzone from '@/components/Dropzone';
import { formatMoney } from '@/lib/format';
import { PAYG_RATE_CENTS } from '@/lib/billing/pricing';

const EXPORT_FORMATS = ['.txt', '.docx', '.srt'];

export default async function Home() {
  const t = await getTranslations('landing');
  const locale = await getLocale();
  const user = await currentUser();
  const titleLines = t('title').split('\n');

  return (
    <main className="page page--wide">
      <section className="hero rise">
        <span className="eyebrow">{t('badge')}</span>
        <h1 className="display">
          {titleLines.map((line, i) => (
            <span key={i}>
              {line}
              {i < titleLines.length - 1 && <br />}
            </span>
          ))}
        </h1>
        <p className="lede">{t('subtitle')}</p>
        <Dropzone signedIn={user !== null} />
      </section>

      <section className="bento" style={{ marginTop: 80 }}>
        <article className="card span-4">
          <h2 className="subtitle">{t('bento.speakersTitle')}</h2>
          <p className="small">{t('bento.speakersBody')}</p>
          <div className="card-body mock" aria-hidden="true">
            <div className="mock-row">
              <span className="mock-speaker">1</span>
              <div className="mock-lines">
                <div className="mock-line" style={{ width: '92%' }} />
                <div className="mock-line" style={{ width: '64%' }} />
              </div>
            </div>
            <div className="mock-row">
              <span className="mock-speaker">2</span>
              <div className="mock-lines">
                <div className="mock-line" style={{ width: '78%' }} />
                <div className="mock-line" style={{ width: '46%' }} />
              </div>
            </div>
          </div>
        </article>

        <article className="card">
          <h2 className="subtitle">{t('bento.exportTitle')}</h2>
          <p className="small">{t('bento.exportBody')}</p>
          <div className="card-body" style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
            {EXPORT_FORMATS.map((format) => (
              <span key={format} className="chip chip--static">
                {format}
              </span>
            ))}
          </div>
        </article>

        <article className="card">
          <h2 className="subtitle">{t('bento.lengthTitle')}</h2>
          <p className="small">{t('bento.lengthBody')}</p>
          <p className="card-body metric">{t('bento.lengthMetric')}</p>
        </article>

        <article className="card">
          <h2 className="subtitle">{t('bento.priceTitle')}</h2>
          <p className="small">{t('bento.priceBody')}</p>
          <p className="card-body metric">
            {formatMoney(PAYG_RATE_CENTS, locale)}
            <span>{t('bento.perMinute')}</span>
          </p>
        </article>

        <article className="card">
          <h2 className="subtitle">{t('bento.languagesTitle')}</h2>
          <p className="small">{t('bento.languagesBody')}</p>
          <div className="card-body" style={{ display: 'flex', gap: 8 }}>
            <span className="chip chip--static">EN</span>
            <span className="chip chip--static">UK</span>
          </div>
        </article>
      </section>
    </main>
  );
}
