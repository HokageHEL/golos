const VERIFY_URL = 'https://challenges.cloudflare.com/turnstile/v0/siteverify';

export async function verifyTurnstile(token: string | null, ip: string): Promise<boolean> {
  if (!token || !process.env.TURNSTILE_SECRET_KEY) return false;

  // Fail closed on any error, including a rejected fetch. An outage at
  // Cloudflare must refuse the anonymous trial, never 500 the upload endpoint —
  // and `!res.ok` alone does not cover DNS failures, refused connections or
  // timeouts, which reject rather than resolve.
  try {
    const res = await fetch(VERIFY_URL, {
      method: 'POST',
      signal: AbortSignal.timeout(10_000),
      headers: { 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({
        secret: process.env.TURNSTILE_SECRET_KEY!,
        response: token,
        remoteip: ip,
      }),
    });

    if (!res.ok) return false;
    const json = (await res.json()) as { success: boolean };
    return json.success === true;
  } catch {
    return false;
  }
}
