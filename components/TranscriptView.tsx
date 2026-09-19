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
    <div className="card card--pad-lg transcript">
      {turns.map((turn, i) => (
        <p key={i} className="turn">
          <span className="turn-time" aria-label={t('timestamp')}>{Math.floor(turn.startMs / 60000)}:{String(Math.floor(turn.startMs / 1000) % 60).padStart(2, '0')}</span>
          {showSpeakers && turn.speaker !== null && (
            <span className="turn-tag">{t('speaker', { n: numbers.get(turn.speaker)! })}:</span>
          )}
          {turn.text}
        </p>
      ))}
    </div>
  );
}
