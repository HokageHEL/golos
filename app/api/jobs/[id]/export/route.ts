import { NextResponse } from 'next/server';
import { getTranslations } from 'next-intl/server';
import { serviceClient } from '@/lib/supabase/server';
import { currentUser } from '@/lib/auth';
import { routing, type Locale } from '@/i18n/routing';
import { toTurns } from '@/lib/transcript/turns';
import { toPlainText } from '@/lib/export/txt';
import { toSrt } from '@/lib/export/srt';
import { toDocx } from '@/lib/export/docx';
import type { Segment } from '@/lib/transcript/types';

type TranscriptWithJob = {
  segments: Segment[];
  jobs: { user_id: string | null; filename: string };
};

/**
 * Filenames come from the user and go straight into a header, so anything
 * that could break or inject one is stripped. `filename*` carries the real
 * name — Cyrillic and friends do not survive the plain `filename` form.
 */
function contentDisposition(base: string, extension: string): string {
  const name = `${base}.${extension}`;
  const ascii = name.replace(/[^\x20-\x7E]/g, '_').replace(/["\\]/g, '_');
  return `attachment; filename="${ascii}"; filename*=UTF-8''${encodeURIComponent(name)}`;
}

export async function GET(request: Request, ctx: { params: Promise<{ id: string }> }) {
  const { id } = await ctx.params;
  const params = new URL(request.url).searchParams;
  const format = params.get('format') ?? 'txt';

  const requestedLocale = params.get('locale');
  const locale: Locale = routing.locales.includes(requestedLocale as Locale)
    ? (requestedLocale as Locale)
    : routing.defaultLocale;

  // Export is the conversion point: viewing is free, taking the file is not.
  const user = await currentUser();
  if (!user) return NextResponse.json({ error: 'sign_in_required' }, { status: 401 });

  const db = serviceClient();
  const { data } = await db
    .from('transcripts')
    .select('segments, jobs!inner(user_id, filename)')
    .eq('job_id', id)
    .single()
    // The client carries no generated `Database` types, so supabase-js assumes
    // the embed could be to-many. It cannot be: `transcripts.job_id` is both
    // the primary key and the foreign key, so PostgREST returns one object.
    .overrideTypes<TranscriptWithJob, { merge: false }>();

  // Anonymous jobs have no user_id, so they can never match — an anonymous
  // transcript is not exportable even by whoever happens to be signed in.
  if (!data || data.jobs.user_id !== user.id) {
    return NextResponse.json({ error: 'not_found' }, { status: 404 });
  }

  const t = await getTranslations({ locale, namespace: 'result' });
  const label = (n: number) => t('speaker', { n });

  const turns = toTurns(data.segments);
  const base = data.jobs.filename.replace(/\.[^.]+$/, '') || 'transcript';

  if (format === 'srt') {
    return new NextResponse(toSrt(turns), {
      headers: {
        'content-type': 'application/x-subrip; charset=utf-8',
        'content-disposition': contentDisposition(base, 'srt'),
      },
    });
  }

  if (format === 'docx') {
    const buffer = await toDocx(turns, label);
    return new NextResponse(new Uint8Array(buffer), {
      headers: {
        'content-type':
          'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
        'content-disposition': contentDisposition(base, 'docx'),
      },
    });
  }

  return new NextResponse(toPlainText(turns, label), {
    headers: {
      'content-type': 'text/plain; charset=utf-8',
      'content-disposition': contentDisposition(base, 'txt'),
    },
  });
}
