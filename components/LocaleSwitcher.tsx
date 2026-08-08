'use client';
import { useLocale } from 'next-intl';
import { usePathname, useRouter, routing } from '@/i18n/routing';

export default function LocaleSwitcher() {
  const locale = useLocale();
  const pathname = usePathname();
  const router = useRouter();

  return (
    <div style={{ display: 'flex', gap: 4, fontSize: 13 }}>
      {routing.locales.map((l) => (
        <button
          key={l}
          onClick={() => router.replace(pathname, { locale: l })}
          style={{
            border: 'none',
            background: 'none',
            cursor: 'pointer',
            padding: '4px 6px',
            fontWeight: l === locale ? 600 : 400,
            color: l === locale ? '#1C1B19' : '#8C897F',
          }}
        >
          {l.toUpperCase()}
        </button>
      ))}
    </div>
  );
}
