import { useTranslations } from 'next-intl';

export default function Home() {
  const t = useTranslations('landing');
  const titleLines = t('title').split('\n');

  return (
    <main style={{ padding: '64px 48px' }}>
      <h1 style={{ fontSize: 48, fontWeight: 700, margin: 0 }}>
        {titleLines.map((line, i) => (
          <span key={i}>
            {line}
            {i < titleLines.length - 1 && <br />}
          </span>
        ))}
      </h1>
      <p style={{ fontSize: 18, color: '#8C897F', marginTop: 16 }}>{t('subtitle')}</p>
    </main>
  );
}
