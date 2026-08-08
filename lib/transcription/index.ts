import { AssemblyAIProvider } from './assemblyai';
import type { TranscriptionProvider } from './types';

let cached: TranscriptionProvider | null = null;

/** The only place the rest of the app gets a provider from. */
export function transcriptionProvider(): TranscriptionProvider {
  if (!cached) {
    cached = new AssemblyAIProvider({
      apiKey: process.env.ASSEMBLYAI_API_KEY!,
      webhookSecret: process.env.ASSEMBLYAI_WEBHOOK_SECRET!,
    });
  }
  return cached;
}
