import type { AiWork, TextExportFormat } from '../api';
import { exportDocument } from './document';

export function exportAiResult(title: string, work: AiWork, format: TextExportFormat): string {
  if (work.segments.length) return exportDocument(title, work.segments, format);
  if (format === 'json') return JSON.stringify({ version: 1, title, kind: work.kind, targetLanguage: work.target || null, model: work.model, createdAt: work.createdAt, content: work.content }, null, 2) + '\n';
  if (format !== 'txt') throw new Error('Untimed generated results support TXT and JSON.');
  return work.content + '\n';
}
