import nextEnv from '@next/env';
const { loadEnvConfig } = nextEnv;
loadEnvConfig(process.cwd(), false);
const required = [
  'NEXT_PUBLIC_SUPABASE_URL', 'NEXT_PUBLIC_SUPABASE_ANON_KEY', 'SUPABASE_SERVICE_ROLE_KEY',
  'NEXT_PUBLIC_APP_URL', 'ASSEMBLYAI_API_KEY', 'ASSEMBLYAI_WEBHOOK_SECRET',
  'UPSTASH_REDIS_REST_URL', 'UPSTASH_REDIS_REST_TOKEN', 'NEXT_PUBLIC_TURNSTILE_SITE_KEY',
  'TURNSTILE_SECRET_KEY', 'POLAR_ACCESS_TOKEN', 'POLAR_WEBHOOK_SECRET',
  'POLAR_TOPUP_PRODUCTS', 'POLAR_SUBSCRIPTION_PRODUCT_ID', 'CRON_SECRET',
];
const errors = required.filter((key) => !process.env[key]?.trim()).map((key) => `${key} is missing`);
for (const key of ['NEXT_PUBLIC_APP_URL', 'NEXT_PUBLIC_SUPABASE_URL', 'UPSTASH_REDIS_REST_URL']) {
  try { if (new URL(process.env[key]).protocol !== 'https:') errors.push(`${key} must use HTTPS for launch`); }
  catch { errors.push(`${key} must be an absolute URL`); }
}
if (process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY?.startsWith('1x0000') || process.env.TURNSTILE_SECRET_KEY?.startsWith('1x0000')) errors.push('Replace Turnstile test keys');
if (process.env.POLAR_SERVER !== 'production') errors.push('POLAR_SERVER must be production for launch');
if (process.env.UPSTASH_REDIS_REST_TOKEN === 'local-dev-token') errors.push('Replace the local Redis token');
for (const key of ['ASSEMBLYAI_WEBHOOK_SECRET', 'CRON_SECRET']) {
  if ((process.env[key]?.length ?? 0) < 32) errors.push(`${key} must have at least 32 random characters`);
}
if (!/^(1000|2500|5000):[0-9a-f-]{36}(,(1000|2500|5000):[0-9a-f-]{36})*$/.test(process.env.POLAR_TOPUP_PRODUCTS || '')) errors.push('POLAR_TOPUP_PRODUCTS must map supported amounts to product UUIDs');
if (errors.length) { console.error(errors.map((e) => `- ${e}`).join('\n')); process.exitCode = 1; }
else console.log('Production environment checks passed (values are never printed).');
