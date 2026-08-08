import type { Turn } from '@/lib/transcript/types';

export type SpeakerLabeller = (index: number) => string;

/** Stable 1-based numbering in order of first appearance. */
export function speakerNumbers(turns: Turn[]): Map<string, number> {
  const numbers = new Map<string, number>();
  for (const t of turns) {
    if (t.speaker !== null && !numbers.has(t.speaker)) {
      numbers.set(t.speaker, numbers.size + 1);
    }
  }
  return numbers;
}

export function toPlainText(turns: Turn[], label: SpeakerLabeller): string {
  const numbers = speakerNumbers(turns);
  const showLabels = numbers.size > 1;

  return turns
    .map((t) => {
      if (!showLabels || t.speaker === null) return t.text;
      return `${label(numbers.get(t.speaker)!)}: ${t.text}`;
    })
    .join('\n\n');
}
