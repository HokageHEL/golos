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
    <nav
      style={{
        position: 'sticky',
        top: 0,
        zIndex: 10,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '24px 48px',
        background: '#FAFAF8F2',
        backdropFilter: 'blur(8px)',
        borderBottom: '1px solid #ECEAE4',
      }}
    >
      <span style={{ fontSize: 19, fontWeight: 700 }}>{t('brand')}</span>
      <div style={{ display: 'flex', alignItems: 'center', gap: 24 }}>
        {links.map(({ href, key }) => {
          const isActive = pathname === href;
          return (
            <Link
              key={href}
              href={href}
              style={{
                fontSize: 14,
                fontWeight: 500,
                color: isActive ? '#1C1B19' : '#8C897F',
                textDecoration: 'none',
                paddingBottom: 4,
                borderBottom: isActive ? '2px solid #1D6F5C' : '2px solid transparent',
              }}
            >
              {t(key)}
            </Link>
          );
        })}
        <LocaleSwitcher />
      </div>
    </nav>
  );
}
