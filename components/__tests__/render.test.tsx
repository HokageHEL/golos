import { expect, test } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { NextIntlClientProvider } from 'next-intl';
import en from '@/messages/en.json';
import uk from '@/messages/uk.json';
import TranscriptView from '../TranscriptView';
import ExportMenu from '../ExportMenu';
import JobProgress from '../JobProgress';
import type { Segment } from '@/lib/transcript/types';

function render(ui: React.ReactElement, locale: 'en' | 'uk' = 'en') {
  return renderToStaticMarkup(
    <NextIntlClientProvider locale={locale} messages={locale === 'uk' ? uk : en}>
      {ui}
    </NextIntlClientProvider>,
  );
}

const seg = (startMs: number, endMs: number, speaker: string | null, text: string): Segment => ({
  startMs,
  endMs,
  speaker,
  text,
});

test('a two-speaker transcript renders labelled turns, not a wall of text', () => {
  const html = render(
    <TranscriptView
      segments={[
        seg(0, 1000, 'A', 'Hello there.'),
        seg(1000, 2000, 'A', 'How are you?'),
        seg(2000, 3000, 'B', 'Fine, thanks.'),
      ]}
    />,
  );

  expect(html).toContain('Speaker 1');
  expect(html).toContain('Speaker 2');
  // The two consecutive A segments became one paragraph.
  expect(html).toContain('Hello there. How are you?');
  expect(html.match(/<p /g)).toHaveLength(2);
});

test('a single speaker gets no labels — they would be noise', () => {
  const html = render(<TranscriptView segments={[seg(0, 1000, 'A', 'Just me talking.')]} />);

  expect(html).not.toContain('Speaker');
  expect(html).toContain('Just me talking.');
});

test('speaker labels follow the reader locale', () => {
  const html = render(
    <TranscriptView segments={[seg(0, 1, 'A', 'Один.'), seg(1, 2, 'B', 'Два.')]} />,
    'uk',
  );

  expect(html).toContain('Спікер 1');
});

test('an owner gets the export control, not the sign-in prompt', () => {
  // The menu itself is closed until clicked, so only the trigger is in the
  // initial markup; the links it opens are covered live against the route.
  expect(render(<ExportMenu jobId="job-1" canExport />)).toContain('Export ▾');
  expect(render(<ExportMenu jobId="job-1" canExport />, 'uk')).toContain('Експорт ▾');
  expect(render(<ExportMenu jobId="job-1" canExport />)).not.toContain('Sign in to export');
});

test('an anonymous viewer is offered sign-in instead of export', () => {
  const html = render(<ExportMenu jobId="job-1" canExport={false} />);

  expect(html).toContain('Sign in to export and save this transcript.');
  expect(html).not.toContain('/api/jobs/job-1/export');
});

test('uploading shows the real percentage', () => {
  const html = render(
    <JobProgress
      phase="uploading"
      uploadFraction={0.42}
      filename="talk.mp3"
      durationSeconds={null}
    />,
  );

  expect(html).toContain('width:42%');
  expect(html).not.toContain('golos-pulse 1.6s');
});

test('transcribing invents no percentage', () => {
  const html = render(
    <JobProgress
      phase="processing"
      uploadFraction={null}
      filename="talk.mp3"
      durationSeconds={null}
    />,
  );

  expect(html).toContain('Transcribing');
  // Full-width but faded and pulsing: a state, not a measurement.
  expect(html).toContain('width:100%');
  expect(html).toContain('golos-pulse 1.6s');
  // No estimate either, because duration is unknown until the provider says so.
  expect(html).not.toContain('Usually about');
});

test('an upload whose reporter is gone falls back to indeterminate, not to zero', () => {
  const html = render(
    <JobProgress
      phase="uploading"
      uploadFraction={null}
      filename="talk.mp3"
      durationSeconds={null}
    />,
  );

  expect(html).not.toContain('width:0%');
  expect(html).toContain('golos-pulse 1.6s');
});

test('the estimate appears only once the duration is known', () => {
  const html = render(
    <JobProgress
      phase="processing"
      uploadFraction={null}
      filename="talk.mp3"
      durationSeconds={1200}
    />,
  );

  expect(html).toContain('Usually about 1 min');
});
