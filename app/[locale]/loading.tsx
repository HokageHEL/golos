import { useTranslations } from 'next-intl';
export default function Loading() {
  const t = useTranslations('job');
  return <main className="page"><p role="status">{t('loading')}</p></main>;
}
