'use client';
import { useTranslations } from 'next-intl';
export default function ErrorPage({ retry }: { retry: () => void }) {
  const t = useTranslations();
  return <main className="page"><div className="card"><h1 className="title">{t('errors.generic')}</h1>
    <button className="btn" onClick={retry}>{t('job.retry')}</button></div></main>;
}
