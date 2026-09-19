import type { Segment } from '@/lib/transcript/types';
import { WEBHOOK_SECRET_HEADER } from './assemblyai';
import { secretMatches } from './secret';
import type { SubmitOptions, TranscriptionProvider, TranscriptionResult } from './types';

/** How long a fake transcription "takes", so the progress view is actually visible. */
const FAKE_LATENCY_MS = 4000;

const SEGMENTS: Segment[] = [
  { startMs: 0, endMs: 3200, speaker: 'A', text: 'Добрий день! Дякую, що знайшли час зустрітися.' },
  { startMs: 3200, endMs: 7400, speaker: 'B', text: 'Вітаю. Радий нарешті поговорити особисто.' },
  { startMs: 7400, endMs: 13100, speaker: 'A', text: 'Почнімо з головного — які терміни ви маєте на увазі?' },
  { startMs: 13100, endMs: 19000, speaker: 'B', text: 'Гадаю, шість тижнів буде достатньо, якщо команда не зміниться.' },
];

/**
 * Stand-in for a real STT vendor so the upload → transcribe → result pipeline
 * can be exercised with no API key, no cost, and no network. Selected only when
 * ASSEMBLYAI_API_KEY is unset outside production — see `./index.ts`.
 *
 * It calls the real webhook rather than short-circuiting to a finished job, so
 * the handler, its idempotency claim, and the ledger all stay on the tested path.
 */
export class FakeProvider implements TranscriptionProvider {
  readonly name = 'fake';

  constructor(private config: { webhookSecret: string }) {}

  async submit(options: SubmitOptions): Promise<{ providerJobId: string }> {
    const providerJobId = `fake-${crypto.randomUUID()}`;

    setTimeout(() => {
      void fetch(options.webhookUrl, {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          [WEBHOOK_SECRET_HEADER]: this.config.webhookSecret,
        },
        body: JSON.stringify({ transcript_id: providerJobId, status: 'completed' }),
      }).catch((error) => {
        // Nothing awaits this, so an unhandled rejection here would be silent.
        console.error('[fake provider] webhook delivery failed', error);
      });
    }, FAKE_LATENCY_MS);

    return { providerJobId };
  }

  async fetchResult(providerJobId: string): Promise<TranscriptionResult> {
    const segments = SEGMENTS;

    return {
      providerJobId,
      status: 'done',
      segments,
      text: segments.map((s) => s.text).join(' '),
      durationSeconds: Math.ceil(segments[segments.length - 1].endMs / 1000),
      language: 'uk',
      error: null,
    };
  }

  verifyWebhook(headers: Headers): boolean {
    return secretMatches(headers.get(WEBHOOK_SECRET_HEADER), this.config.webhookSecret);
  }
}
