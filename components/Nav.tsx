'use client';
import { useTranslations } from 'next-intl';
import { Link, usePathname } from '@/i18n/routing';
import LocaleSwitcher from '@/components/LocaleSwitcher';

const links = [
  { href: '/', key: 'home' as const },
  { href: '/pricing', key: 'pricing' as const },
  { href: '/dashboard', key: 'dashboard' as const },
];

export default function Nav() {
  const t = useTranslations('nav');
  const pathname = usePathname();

  return (
    <nav className="nav">
      <Link href="/" className="nav-brand">
        {t('brand')}
      </Link>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <div className="nav-links">
          {links.map(({ href, key }) => (
            <Link
              key={href}
              href={href}
              className="nav-link"
              aria-current={pathname === href ? 'page' : undefined}
            >
              {t(key)}
            </Link>
          ))}
        </div>
        <LocaleSwitcher />
      </div>
    </nav>
  );
}
