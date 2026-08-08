const SUPPORTED = new Set([
  'audio/mpeg',
  'audio/mp3',
  'audio/wav',
  'audio/x-wav',
  'audio/m4a',
  'audio/x-m4a',
  'audio/mp4',
  'audio/aac',
  'audio/ogg',
  'audio/webm',
  'video/mp4',
]);

export function isSupportedAudio(mimeType: string): boolean {
  return SUPPORTED.has(mimeType);
}

/** Everything for a job lives under its id, so deleting a job deletes its media. */
export function storagePathFor(jobId: string, filename: string): string {
  const match = /\.([A-Za-z0-9]+)$/.exec(filename);
  const ext = match ? match[1].toLowerCase() : 'bin';
  return `${jobId}/source.${ext}`;
}
