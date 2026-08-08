import type { Segment } from '@/lib/transcript/types';

export type SubmitOptions = {
  audioUrl: string;
  webhookUrl: string;
  /** Hard cap in ms. Used to bound the cost of anonymous trials. */
  maxDurationMs?: number;
};

export type TranscriptionResult = {
  providerJobId: string;
  status: 'done' | 'failed';
  segments: Segment[];
  text: string;
  durationSeconds: number;
  language: string | null;
  error: string | null;
};

export interface TranscriptionProvider {
  readonly name: string;
  submit(options: SubmitOptions): Promise<{ providerJobId: string }>;
  fetchResult(providerJobId: string): Promise<TranscriptionResult>;
  verifyWebhook(headers: Headers): boolean;
}
