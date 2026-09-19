import { NextResponse } from 'next/server';
import { sessionClient } from '@/lib/auth';
import { claimTrials } from '@/lib/jobs/ownership';
import { appUrl } from '@/lib/api';

export async function GET(request: Request) {
  const url = new URL(request.url);
  const prefix = url.searchParams.get('locale') === 'uk' ? '/uk' : '';
  const origin = appUrl();
  const code = url.searchParams.get('code');
  if (code) {
    const supabase = await sessionClient();
    const { data, error } = await supabase.auth.exchangeCodeForSession(code);
    if (!error && data.user) {
      await claimTrials(data.user.id);
      return NextResponse.redirect(`${origin}${prefix}/dashboard`);
    }
  }
  return NextResponse.redirect(`${origin}${prefix}/login?error=link_expired`);
}
