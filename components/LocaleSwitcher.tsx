'use client';
import { useLocale } from 'next-intl';
import { usePathname, useRouter, routing } from '@/i18n/routing';

export default function LocaleSwitcher() {
  const locale = useLocale();
  const pathname = usePathname();
  const router = useRouter();

  return (
    <div className="segmented">
      {routing.locales.map((l) => (
        <button
          key={l}
          type="button"
          aria-pressed={l === locale}
          onClick={() => router.replace(`${pathname}${window.location.search}`, { locale: l })}
        >
          {l.toUpperCase()}
        </button>
      ))}
    </div>
  );
}
