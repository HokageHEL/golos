import { AssemblyAIProvider } from './assemblyai';
import { FakeProvider } from './fake';
import type { TranscriptionProvider } from './types';

let cached: TranscriptionProvider | null = null;

/** The only place the rest of the app gets a provider from. */
export function transcriptionProvider(): TranscriptionProvider {
  if (!cached) {
    const apiKey = process.env.ASSEMBLYAI_API_KEY;

    // A missing key in production is a misconfiguration, not an invitation to
    // serve invented transcripts — only local dev falls back to the fake.
    if (!apiKey && process.env.NODE_ENV !== 'production') {
      console.warn('[transcription] ASSEMBLYAI_API_KEY unset — using the fake provider');
      cached = new FakeProvider({
        // Empty buffers compare equal, so an unset secret would accept any
        // caller. Fall back to a non-empty constant instead.
        webhookSecret: process.env.ASSEMBLYAI_WEBHOOK_SECRET || 'local-dev-webhook-secret',
      });
    } else {
      if (!apiKey || !process.env.ASSEMBLYAI_WEBHOOK_SECRET) {
        throw new Error('ASSEMBLYAI_API_KEY and ASSEMBLYAI_WEBHOOK_SECRET are required');
      }
      cached = new AssemblyAIProvider({
        apiKey: apiKey!,
        webhookSecret: process.env.ASSEMBLYAI_WEBHOOK_SECRET!,
      });
    }
  }
  return cached;
}
