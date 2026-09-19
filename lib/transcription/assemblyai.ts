import type { Segment } from '@/lib/transcript/types';
import { secretMatches } from './secret';
import type { SubmitOptions, TranscriptionProvider, TranscriptionResult } from './types';

const API = 'https://api.assemblyai.com/v2/transcript';
export const WEBHOOK_SECRET_HEADER = 'x-golos-webhook-secret';

type Utterance = { start: number; end: number; speaker: string | null; text: string };

export class AssemblyAIProvider implements TranscriptionProvider {
  readonly name = 'assemblyai';

  constructor(private config: { apiKey: string; webhookSecret: string }) {}

  private headers() {
    return { authorization: this.config.apiKey, 'content-type': 'application/json' };
  }

  async submit(options: SubmitOptions): Promise<{ providerJobId: string }> {
    const body: Record<string, unknown> = {
      audio_url: options.audioUrl,
      speaker_labels: true,
      language_detection: true,
      speech_models: ['universal-2'],
      webhook_url: options.webhookUrl,
      webhook_auth_header_name: WEBHOOK_SECRET_HEADER,
      webhook_auth_header_value: this.config.webhookSecret,
    };

    if (options.maxDurationMs !== undefined) body.audio_end_at = options.maxDurationMs;

    const res = await fetch(API, {
      method: 'POST',
      headers: this.headers(),
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(20_000),
    });

    if (!res.ok) throw new Error(`AssemblyAI submit failed: ${res.status} ${await res.text()}`);

    const json = (await res.json()) as { id: string };
    if (!json.id) throw new Error('AssemblyAI did not return a job ID');
    return { providerJobId: json.id };
  }

  async fetchResult(providerJobId: string): Promise<TranscriptionResult | null> {
    const res = await fetch(`${API}/${encodeURIComponent(providerJobId)}`, {
      headers: this.headers(), signal: AbortSignal.timeout(20_000), cache: 'no-store',
    });
    if (!res.ok) throw new Error(`AssemblyAI fetch failed: ${res.status}`);

    const json = (await res.json()) as {
      id: string;
      status: string;
      text?: string;
      audio_duration?: number;
      language_code?: string;
      error?: string | null;
      utterances?: Utterance[] | null;
    };

    if (json.status === 'error') {
      return {
        providerJobId: json.id,
        status: 'failed',
        segments: [],
        text: '',
        durationSeconds: 0,
        language: null,
        error: json.error ?? 'Unknown provider error',
      };
    }

    if (json.status === 'queued' || json.status === 'processing') return null;
    if (json.status !== 'completed' || !Number.isFinite(json.audio_duration) || json.audio_duration! < 0) {
      throw new Error('Invalid AssemblyAI result');
    }
    const segments: Segment[] = (json.utterances ?? []).map((u) => ({
      startMs: u.start,
      endMs: u.end,
      speaker: u.speaker,
      text: u.text,
    }));
    if (segments.length === 0 && json.text?.trim()) {
      segments.push({ startMs: 0, endMs: Math.round(json.audio_duration! * 1000), speaker: null, text: json.text });
    }

    return {
      providerJobId: json.id,
      status: 'done',
      segments,
      text: json.text ?? '',
      durationSeconds: Math.ceil(json.audio_duration!),
      language: json.language_code ?? null,
      error: null,
    };
  }

  verifyWebhook(headers: Headers): boolean {
    return secretMatches(headers.get(WEBHOOK_SECRET_HEADER), this.config.webhookSecret);
  }
}
