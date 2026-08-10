/**
 * Upload progress lives outside React because the component that starts the
 * upload (the landing dropzone) unmounts the instant we navigate to /j/[id],
 * while the XHR keeps running — it is a plain promise, not tied to the React
 * lifecycle. Module state survives client-side navigation, so the job page
 * picks the fraction up where the dropzone left it.
 *
 * A hard refresh loses it. The job page then falls back to the indeterminate
 * bar, which is honest: at that point we genuinely no longer know.
 */

export type UploadFailure = 'upload' | 'start';

export type UploadState = {
  fraction: number;
  failure: UploadFailure | null;
};

const states = new Map<string, UploadState>();
const listeners = new Map<string, Set<() => void>>();

/**
 * A fresh object every time: `useSyncExternalStore` compares snapshots by
 * identity, so mutating in place would go unnoticed.
 */
function publish(jobId: string, state: UploadState): void {
  states.set(jobId, state);
  for (const listener of listeners.get(jobId) ?? []) listener();
}

export function beginUpload(jobId: string): void {
  publish(jobId, { fraction: 0, failure: null });
}

export function reportProgress(jobId: string, fraction: number): void {
  publish(jobId, { fraction, failure: null });
}

export function reportFailure(jobId: string, failure: UploadFailure): void {
  publish(jobId, { fraction: states.get(jobId)?.fraction ?? 0, failure });
}

export function uploadState(jobId: string): UploadState | undefined {
  return states.get(jobId);
}

export function subscribeToUpload(jobId: string, listener: () => void): () => void {
  const set = listeners.get(jobId) ?? new Set<() => void>();
  set.add(listener);
  listeners.set(jobId, set);

  return () => {
    set.delete(listener);
    if (set.size === 0) listeners.delete(jobId);
  };
}
