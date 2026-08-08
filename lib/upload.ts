export type UploadProgress = (fraction: number) => void;

/**
 * XMLHttpRequest rather than fetch: fetch still has no upload progress events,
 * and the progress bar is the only honest feedback during a 120 MB upload.
 */
export function uploadToStorage(
  url: string,
  file: File,
  onProgress: UploadProgress,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('PUT', url);
    xhr.setRequestHeader('content-type', file.type);

    xhr.upload.onprogress = (event) => {
      if (event.lengthComputable) onProgress(event.loaded / event.total);
    };
    xhr.onload = () =>
      xhr.status >= 200 && xhr.status < 300
        ? resolve()
        : reject(new Error(`upload failed: ${xhr.status}`));
    xhr.onerror = () => reject(new Error('upload failed: network'));

    xhr.send(file);
  });
}
