import { getTranslations } from 'next-intl/server';
import { Link } from '@/i18n/routing';

export default async function Footer() {
  const t = await getTranslations();

  return (
    <footer className="footer">
      <span style={{ fontWeight: 600, color: 'var(--ink)' }}>{t('nav.brand')}</span>
      <span>{t('footer.tagline')}</span>
      <Link href="/pricing">{t('nav.pricing')}</Link>
      <Link href="/login">{t('login.title')}</Link>
    </footer>
  );
}
