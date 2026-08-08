import { Document, Packer, Paragraph, TextRun } from 'docx';
import type { Turn } from '@/lib/transcript/types';
import { speakerNumbers, type SpeakerLabeller } from './txt';

export async function toDocx(turns: Turn[], label: SpeakerLabeller): Promise<Buffer> {
  const numbers = speakerNumbers(turns);
  const showLabels = numbers.size > 1;

  const paragraphs = turns.map((t) => {
    const runs: TextRun[] = [];
    if (showLabels && t.speaker !== null) {
      runs.push(new TextRun({ text: `${label(numbers.get(t.speaker)!)}: `, bold: true }));
    }
    runs.push(new TextRun(t.text));
    return new Paragraph({ children: runs, spacing: { after: 200 } });
  });

  return Packer.toBuffer(new Document({ sections: [{ children: paragraphs }] }));
}
