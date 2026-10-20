import type { Segment } from '../transcript/types';
import { displaySpeaker, exportWithSpeakers } from '../speakerLabels';
import { toSrt } from './srt';
import { toVtt } from './vtt';
import type { TextExportFormat } from '../api';
import {
  markdownHeader,
  plainTimestampHeader,
  speakerLabel,
  timestamp,
  transcriptBlocks,
} from './layout';
import {
  validateSubtitleOptions,
  validateTranscriptLayout,
  type SubtitleOptions,
  type TranscriptLayoutOptions,
} from './options';

export type DocumentExportOptions = {
  /** Applies to TXT, MD and HTML. CSV/JSON always keep canonical data. */
  layout?: TranscriptLayoutOptions;
  /** Applies to SRT and VTT only. */
  subtitles?: SubtitleOptions;
};

const html = (text: string) => text.replace(/[&<>"']/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[ch]!);
const markdown = (text: string) => text.replace(/[\\`*_{}[\]<>#+.!|~-]/g, '\\$&');
// Quote every field and neutralize spreadsheet formulas, even after whitespace.
const csv = (text: string) => `"${(/^[\s]*[=+\-@]/.test(text) ? `'${text}` : text).replaceAll('"', '""')}"`;

function readableText(segments: Segment[], layout: TranscriptLayoutOptions): string {
  return transcriptBlocks(segments, layout.grouping).map(block => {
    let prefix = '';
    if (layout.includeTimestamps) prefix += `${plainTimestampHeader(block)} `;
    if (layout.includeSpeakers) {
      const label = speakerLabel(block.speaker);
      if (label) prefix += `${label}: `;
    }
    return prefix + block.text;
  }).join('\n\n');
}

function readableMarkdown(title: string, segments: Segment[], layout: TranscriptLayoutOptions): string {
  const body = transcriptBlocks(segments, layout.grouping).map(block => {
    const header = markdownHeader(block, layout.includeTimestamps, layout.includeSpeakers);
    return header ? `**${markdown(header)}**\n\n${markdown(block.text)}` : markdown(block.text);
  }).join('\n\n');
  return `# ${markdown(title).replaceAll('\n', ' ')}\n\n${body}${body ? '\n' : ''}`;
}

function readableHtml(title: string, segments: Segment[], layout: TranscriptLayoutOptions): string {
  const sections = transcriptBlocks(segments, layout.grouping).map(block => {
    const header = markdownHeader(block, layout.includeTimestamps, layout.includeSpeakers);
    return `<section>${header ? `<header>${html(header)}</header>` : ''}<p>${html(block.text)}</p></section>`;
  }).join('');
  return `<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><title>${html(title)}</title><style>body{font:17px/1.6 system-ui,sans-serif;max-width:52rem;margin:3rem auto;padding:0 1.5rem;color:#171717}h1{line-height:1.2}section{break-inside:avoid;margin:1.5rem 0}header{font-size:.85em;color:#555}p{white-space:pre-wrap;margin:.3rem 0}@media print{body{margin:0;max-width:none}}</style></head><body><h1>${html(title)}</h1>${sections}</body></html>\n`;
}

/**
 * Render a transcript export. Calling without options preserves the existing
 * outputs exactly. CSV and JSON never take layout options: they keep canonical
 * segment order, timestamps, speakers and text for reliable re-import.
 */
export function exportDocument(
  title: string,
  segments: Segment[],
  format: TextExportFormat,
  options: DocumentExportOptions = {},
): string {
  const layout = options.layout ? validateTranscriptLayout(options.layout) : null;
  const subtitles = options.subtitles ? validateSubtitleOptions(options.subtitles) : null;

  if (format === 'txt') return layout ? readableText(segments, layout) : exportWithSpeakers(segments, format);
  if (format === 'srt') {
    if (!subtitles) return exportWithSpeakers(segments, format);
    const labeled = segments.map(segment => ({
      ...segment,
      text: segment.speaker ? `${displaySpeaker(segment.speaker)}: ${segment.text}` : segment.text,
    }));
    return toSrt(labeled, subtitles);
  }
  if (format === 'vtt') return subtitles ? toVtt(segments, subtitles) : toVtt(segments);
  if (format === 'json') return JSON.stringify({ version: 1, title, timestampUnit: 'milliseconds', segments }, null, 2) + '\n';
  if (format === 'csv') return '\uFEFF' + [
    ['start_ms', 'end_ms', 'speaker', 'text'].map(csv).join(';'),
    ...segments.map(segment => [String(segment.startMs), String(segment.endMs), displaySpeaker(segment.speaker), segment.text].map(csv).join(';')),
  ].join('\r\n') + '\r\n';
  if (format === 'md') {
    if (layout) return readableMarkdown(title, segments, layout);
    return `# ${markdown(title).replaceAll('\n', ' ')}\n\n` + segments.map(segment =>
      `**${timestamp(segment.startMs)} – ${timestamp(segment.endMs)}${segment.speaker ? ` · ${markdown(displaySpeaker(segment.speaker))}` : ''}**\n\n${markdown(segment.text)}\n`,
    ).join('\n');
  }
  if (layout) return readableHtml(title, segments, layout);
  return `<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><title>${html(title)}</title><style>body{font:17px/1.6 system-ui,sans-serif;max-width:52rem;margin:3rem auto;padding:0 1.5rem;color:#171717}h1{line-height:1.2}section{break-inside:avoid;margin:1.5rem 0}header{font-size:.85em;color:#555}p{white-space:pre-wrap;margin:.3rem 0}@media print{body{margin:0;max-width:none}}</style></head><body><h1>${html(title)}</h1>${segments.map(segment =>
    `<section><header>${timestamp(segment.startMs)} – ${timestamp(segment.endMs)}${segment.speaker ? ` · ${html(displaySpeaker(segment.speaker))}` : ''}</header><p>${html(segment.text)}</p></section>`,
  ).join('')}</body></html>\n`;
}
