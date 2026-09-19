import { useTranslations } from 'next-intl';
import { Link } from '@/i18n/routing';
export default function NotFound() {
  const t = useTranslations();
  return <main className="page"><h1 className="title">{t('job.notFound')}</h1><Link href="/">{t('nav.home')}</Link></main>;
}
