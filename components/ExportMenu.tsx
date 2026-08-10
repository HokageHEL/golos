'use client';

import { useState } from 'react';
import { useLocale, useTranslations } from 'next-intl';
import { Link } from '@/i18n/routing';

const FORMATS = ['txt', 'docx', 'srt'] as const;

export default function ExportMenu({ jobId, canExport }: { jobId: string; canExport: boolean }) {
  const t = useTranslations();
  const locale = useLocale();
  const [isOpen, setIsOpen] = useState(false);

  // Viewing is free; taking the file is the conversion point.
  if (!canExport) {
    return (
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ fontSize: 14, color: '#8C897F' }}>{t('trial.signInToExport')}</span>
        <Link
          href="/login"
          style={{
            padding: '9px 18px',
            borderRadius: 10,
            background: '#1C1B19',
            color: '#fff',
            fontSize: 14,
            fontWeight: 600,
          }}
        >
          {t('login.title')}
        </Link>
      </div>
    );
  }

  return (
    <div style={{ position: 'relative' }}>
      <button
        type="button"
        onClick={() => setIsOpen((open) => !open)}
        style={{
          padding: '9px 18px',
          borderRadius: 10,
          border: '1px solid #ECEAE4',
          background: '#fff',
          fontSize: 14,
          fontWeight: 600,
          color: '#1C1B19',
          cursor: 'pointer',
        }}
      >
        {t('result.export')} ▾
      </button>

      {isOpen && (
        <>
          {/* Catches the click that should dismiss the menu, without a
              document-level listener that would fire before onClick above. */}
          <div
            onClick={() => setIsOpen(false)}
            style={{ position: 'fixed', inset: 0, zIndex: 20 }}
          />
          <div
            style={{
              position: 'absolute',
              top: 'calc(100% + 8px)',
              right: 0,
              zIndex: 21,
              minWidth: 160,
              padding: 6,
              background: '#fff',
              border: '1px solid #ECEAE4',
              borderRadius: 12,
              boxShadow: '0 12px 32px rgba(0,0,0,0.10)',
              animation: 'golos-fade-up 0.16s ease',
            }}
          >
            {FORMATS.map((format) => (
              <a
                key={format}
                // The file's speaker labels are written in the language the
                // reader is looking at, not the server's default.
                href={`/api/jobs/${jobId}/export?format=${format}&locale=${locale}`}
                onClick={() => setIsOpen(false)}
                style={{
                  display: 'block',
                  padding: '9px 12px',
                  borderRadius: 8,
                  fontSize: 14,
                  color: '#1C1B19',
                }}
              >
                .{format}
              </a>
            ))}
          </div>
        </>
      )}

      <style>{`
        @keyframes golos-fade-up {
          from { transform: translateY(-4px); opacity: 0; }
          to { transform: translateY(0); opacity: 1; }
        }
      `}</style>
    </div>
  );
}
