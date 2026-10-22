/** Layout choices for human-readable transcript exports (TXT, MD, HTML, DOCX). */
export type SpeakerGrouping = 'paragraph' | 'same-speaker';

export type TranscriptLayoutOptions = {
  includeTimestamps: boolean;
  includeSpeakers: boolean;
  grouping: SpeakerGrouping;
};

/** Subtitle-only choices. These never change canonical transcript data. */
export type SubtitleOptions = {
  /** Added to every cue timestamp. Negative offsets trim leading silence. */
  offsetMs: number;
  /** Maximum grapheme clusters per subtitle line; 0 disables wrapping. */
  maxLineLength: number;
};

export type ExportOptionsState = TranscriptLayoutOptions & {
  /** Unparsed draft keeps incomplete signed input intact until export. */
  offsetText: string;
  maxLineLength: number;
  /** Defaults off so existing exports and every non-manual flow stay unchanged. */
  custom: boolean;
};

export const DEFAULT_TRANSCRIPT_LAYOUT: TranscriptLayoutOptions = {
  includeTimestamps: true,
  includeSpeakers: true,
  grouping: 'paragraph',
};

export const DEFAULT_SUBTITLE_OPTIONS: SubtitleOptions = {
  offsetMs: 0,
  maxLineLength: 0,
};

export const DEFAULT_EXPORT_OPTIONS: ExportOptionsState = {
  custom: false,
  ...DEFAULT_TRANSCRIPT_LAYOUT,
  offsetText: String(DEFAULT_SUBTITLE_OPTIONS.offsetMs),
  maxLineLength: DEFAULT_SUBTITLE_OPTIONS.maxLineLength,
};

/** SRT/VTT timestamp fields are HH:MM:SS,mmm (two-digit hours). */
export const MAX_SUBTITLE_MS = 99 * 3_600_000 + 59 * 60_000 + 59_999;

export function validateTranscriptLayout(options: TranscriptLayoutOptions): TranscriptLayoutOptions {
  if (options.grouping !== 'paragraph' && options.grouping !== 'same-speaker') {
    throw new Error('Choose paragraph or adjacent same-speaker export grouping.');
  }
  if (typeof options.includeTimestamps !== 'boolean' || typeof options.includeSpeakers !== 'boolean') {
    throw new Error('Invalid transcript export layout.');
  }
  return {
    includeTimestamps: options.includeTimestamps,
    includeSpeakers: options.includeSpeakers,
    grouping: options.grouping,
  };
}

export function validateSubtitleOptions(options: SubtitleOptions): SubtitleOptions {
  if (!Number.isSafeInteger(options.offsetMs)) {
    throw new Error('Subtitle timestamp offset must be a whole, finite number of milliseconds.');
  }
  if (!Number.isInteger(options.maxLineLength) || options.maxLineLength < 0 || options.maxLineLength > 200) {
    throw new Error('Subtitle line width must be between 0 and 200 characters.');
  }
  return { offsetMs: options.offsetMs, maxLineLength: options.maxLineLength };
}

/** Only the three layout fields cross into the native DOCX command. */
export function docxLayoutOptions(options: TranscriptLayoutOptions): TranscriptLayoutOptions {
  return validateTranscriptLayout(options);
}


/** Parse the editor draft only when exporting, never while a sign is typed. */
export function parseSubtitleOptions(draft: Pick<ExportOptionsState, 'offsetText' | 'maxLineLength'>): SubtitleOptions {
  const text = draft.offsetText.trim();
  if (!/^[+-]?\d+$/.test(text)) {
    throw new Error('Subtitle timestamp offset must be a whole, finite number of milliseconds.');
  }
  return validateSubtitleOptions({ offsetMs: Number(text), maxLineLength: draft.maxLineLength });
}
