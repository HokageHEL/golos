import type { Segment } from '../transcript/types';
import { displaySpeaker, exportWithSpeakers } from '../speakerLabels';
import { toVtt } from './vtt';
import type { TextExportFormat } from '../api';

function timestamp(ms: number): string {
  const value = Math.max(0, Math.round(ms));
  return `${String(Math.floor(value / 3600000)).padStart(2, '0')}:${String(Math.floor(value / 60000) % 60).padStart(2, '0')}:${String(Math.floor(value / 1000) % 60).padStart(2, '0')}.${String(value % 1000).padStart(3, '0')}`;
}
const html = (text: string) => text.replace(/[&<>"']/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[ch]!);
const markdown = (text: string) => text.replace(/[\\`*_{}[\]<>#+.!|~-]/g, '\\$&');
// Quote every field and neutralize spreadsheet formulas, even after whitespace.
const csv = (text: string) => `"${(/^[\s]*[=+\-@]/.test(text) ? `'${text}` : text).replaceAll('"', '""')}"`;

export function exportDocument(title: string, segments: Segment[], format: TextExportFormat): string {
  if (format === 'txt' || format === 'srt') return exportWithSpeakers(segments, format);
  if (format === 'vtt') return toVtt(segments);
  if (format === 'json') return JSON.stringify({ version: 1, title, timestampUnit: 'milliseconds', segments }, null, 2) + '\n';
  if (format === 'csv') return '\uFEFF' + [
    ['start_ms', 'end_ms', 'speaker', 'text'].map(csv).join(';'),
    ...segments.map(segment => [String(segment.startMs), String(segment.endMs), displaySpeaker(segment.speaker), segment.text].map(csv).join(';')),
  ].join('\r\n') + '\r\n';
  if (format === 'md') return `# ${markdown(title).replaceAll('\n', ' ')}\n\n` + segments.map(segment =>
    `**${timestamp(segment.startMs)} – ${timestamp(segment.endMs)}${segment.speaker ? ` · ${markdown(displaySpeaker(segment.speaker))}` : ''}**\n\n${markdown(segment.text)}\n`,
  ).join('\n');
  return `<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><title>${html(title)}</title><style>body{font:17px/1.6 system-ui,sans-serif;max-width:52rem;margin:3rem auto;padding:0 1.5rem;color:#171717}h1{line-height:1.2}section{break-inside:avoid;margin:1.5rem 0}header{font-size:.85em;color:#555}p{white-space:pre-wrap;margin:.3rem 0}@media print{body{margin:0;max-width:none}}</style></head><body><h1>${html(title)}</h1>${segments.map(segment =>
    `<section><header>${timestamp(segment.startMs)} – ${timestamp(segment.endMs)}${segment.speaker ? ` · ${html(displaySpeaker(segment.speaker))}` : ''}</header><p>${html(segment.text)}</p></section>`,
  ).join('')}</body></html>\n`;
}
