'use client';
import { useEffect } from 'react';
import { useRouter } from '@/i18n/routing';

/** Refresh briefly while a payment webhook or an active job can change history. */
export default function DashboardRefresh({ enabled }: { enabled: boolean }) {
  const router = useRouter();
  useEffect(() => {
    if (!enabled) return;
    let remaining = 24;
    const timer = setInterval(() => {
      if (document.visibilityState === 'visible') router.refresh();
      if (--remaining <= 0) clearInterval(timer);
    }, 5000);
    return () => clearInterval(timer);
  }, [enabled, router]);
  return null;
}
