import { serviceClient } from '@/lib/supabase/server';
import { transcriptionProvider } from '@/lib/transcription';
import { wordCount } from '@/lib/transcript/turns';

/** One database transaction owns the transcript, debit, plan usage, and status. */
export async function applyCompletion(providerJobId: string): Promise<boolean> {
  const result = await transcriptionProvider().fetchResult(providerJobId);
  if (!result) return false;
  const { data, error } = await serviceClient().rpc('complete_job', {
    p_provider_id: providerJobId,
    p_result: { ...result, wordCount: wordCount(result.text) },
  });
  if (error) throw error;
  return data === true;
}
