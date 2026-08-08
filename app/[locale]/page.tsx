import { useTranslations } from 'next-intl';
import Dropzone from '@/components/Dropzone';

export default function Home() {
  const t = useTranslations('landing');
  const titleLines = t('title').split('\n');

  return (
    <main
      style={{
        padding: '64px 48px',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        textAlign: 'center',
      }}
    >
      <h1 style={{ fontSize: 48, fontWeight: 700, margin: 0 }}>
        {titleLines.map((line, i) => (
          <span key={i}>
            {line}
            {i < titleLines.length - 1 && <br />}
          </span>
        ))}
      </h1>
      <p style={{ fontSize: 18, color: '#8C897F', marginTop: 16, marginBottom: 40 }}>
        {t('subtitle')}
      </p>
      <Dropzone />
    </main>
  );
}
