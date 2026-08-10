'use client';

import { useTranslations } from 'next-intl';
import { toTurns } from '@/lib/transcript/turns';
import { speakerNumbers } from '@/lib/export/txt';
import type { Segment } from '@/lib/transcript/types';

export default function TranscriptView({ segments }: { segments: Segment[] }) {
  const t = useTranslations('result');
  const turns = toTurns(segments);
  const numbers = speakerNumbers(turns);

  // One speaker means diarization found nothing worth labelling; prefixing
  // every paragraph with the same name is noise.
  const showSpeakers = numbers.size > 1;

  return (
    <div
      contentEditable
      suppressContentEditableWarning
      style={{
        fontSize: 18,
        lineHeight: 1.75,
        background: '#fff',
        border: '1px solid #ECEAE4',
        borderRadius: 16,
        padding: 32,
        minHeight: 280,
        outline: 'none',
      }}
    >
      {turns.map((turn, i) => (
        <p key={i} style={{ marginBottom: 20 }}>
          {showSpeakers && turn.speaker !== null && (
            <span style={{ fontWeight: 600, color: '#1D6F5C', marginRight: 8 }}>
              {t('speaker', { n: numbers.get(turn.speaker)! })}:
            </span>
          )}
          {turn.text}
        </p>
      ))}
    </div>
  );
}
