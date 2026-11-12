const storageKey = 'golos.transcription-preferences';

export type TranscriptionPreferences = {
  language: 'auto' | 'en' | 'uk';
  autoSpeakers: boolean;
  speakerCount: 'auto' | number;
};

export const defaultPreferences: TranscriptionPreferences = {
  language: 'auto',
  autoSpeakers: true,
  speakerCount: 'auto',
};

const supportedSpeakerCounts = new Set([1, 2, 3, 4, 5, 6, 8, 10, 20]);

function validatePreferences(value: unknown): TranscriptionPreferences {
  if (!value || typeof value !== 'object') return { ...defaultPreferences };
  const candidate = value as Record<string, unknown>;
  return {
    language: candidate.language === 'en' || candidate.language === 'uk' ? candidate.language : 'auto',
    autoSpeakers: typeof candidate.autoSpeakers === 'boolean' ? candidate.autoSpeakers : true,
    speakerCount: candidate.speakerCount === 'auto' ||
      (typeof candidate.speakerCount === 'number' && supportedSpeakerCounts.has(candidate.speakerCount))
      ? candidate.speakerCount : 'auto',
  };
}

export function readPreferences(): TranscriptionPreferences {
  try {
    const raw = localStorage.getItem(storageKey);
    return raw === null ? { ...defaultPreferences } : validatePreferences(JSON.parse(raw));
  } catch {
    return { ...defaultPreferences };
  }
}

export function savePreferences(preferences: TranscriptionPreferences): void {
  try {
    localStorage.setItem(storageKey, JSON.stringify(validatePreferences(preferences)));
  } catch {
    // Preferences still work for the current session when storage is unavailable.
  }
}
