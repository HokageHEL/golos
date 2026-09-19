# Golos

Audio transcription with speaker labels, editable transcripts, TXT/DOCX/SRT exports,
English/Ukrainian UI, anonymous trials, prepaid credit, and monthly subscriptions.
Built with Next.js 16.3.5, Supabase, AssemblyAI, Upstash, Cloudflare Turnstile, and Polar.

## Local setup

Prerequisites: Node 22+, npm, Docker Desktop running.

```bash
npm ci
cp .env.example .env.local
npx supabase start
npx supabase migration up --local
docker compose up -d
```

Copy the public anon key and private service-role key printed by Supabase into
`.env.local`. Keep the local URLs and public Turnstile test keys from `.env.example`.
Set a random `CRON_SECRET` for local cron testing. Never commit `.env.local`.

```bash
npm run dev
```

Open <http://localhost:3000>. Supabase Studio is at <http://127.0.0.1:54323> and
Mailpit at <http://127.0.0.1:54324>. Local magic links arrive in Mailpit, not a real inbox.
Sign-in uses PKCE: open the email link in the browser that requested it.

With `ASSEMBLYAI_API_KEY` blank, **development only** uses a canned transcript and
calls the real local webhook after four seconds. The test provider requires no paid
service. Production refuses to transcribe without real AssemblyAI credentials.
Turnstile verification still calls Cloudflare, including with its public test keys.

If port 3000 is occupied, use a matching origin and port:

```bash
NEXT_PUBLIC_APP_URL=http://localhost:3100 npx next dev --hostname localhost --port 3100
npm run test:smoke -- http://localhost:3100
```

Use `localhost`, or the default Next.js hostname. Explicit `--hostname 127.0.0.1`
can trigger a Next.js internal rewrite loop; see [the upstream issue](https://github.com/vercel/next.js/issues/94745).
Add any changed origin to Supabase Auth's redirect allow-list before testing login.

## Verification

```bash
npm run lint             # zero warnings allowed
npm run typecheck
npm test
npm run test:db          # running local Supabase; fixtures roll back
npm run test:smoke       # running development app, Supabase, Redis; fake provider only
npm audit
npm run build
npm start
```

`test:smoke` creates and removes its own local test account and synthetic WAV files.
It verifies sign-in, upload, concurrent starts, completion, one debit, authorization,
edits/conflicts, all three exports, deletion, anonymous quota, and account claiming.
It uses the public Turnstile test keys and reserves one trial in local Redis.
It refuses hosted databases or a configured paid transcription provider.

`test:db` verifies funded limits, idempotent starts/completions, plan consumption,
subscription replay handling, private RPC permissions, and cross-account RLS.
It does not reset existing database data.

## Required environment variables

[.env.example](.env.example) is the complete, commented inventory. The launch check
prints variable names and problems, never secret values:

```bash
npm run check:env
```

| Purpose | Variables |
| --- | --- |
| Supabase | `NEXT_PUBLIC_SUPABASE_URL`, `NEXT_PUBLIC_SUPABASE_ANON_KEY`, `SUPABASE_SERVICE_ROLE_KEY` |
| Canonical site origin | `NEXT_PUBLIC_APP_URL` |
| Transcription | `ASSEMBLYAI_API_KEY`, `ASSEMBLYAI_WEBHOOK_SECRET` |
| Anonymous quota | `UPSTASH_REDIS_REST_URL`, `UPSTASH_REDIS_REST_TOKEN` |
| Bot protection | `NEXT_PUBLIC_TURNSTILE_SITE_KEY`, `TURNSTILE_SECRET_KEY` |
| Payments | `POLAR_ACCESS_TOKEN`, `POLAR_WEBHOOK_SECRET`, `POLAR_TOPUP_PRODUCTS`, `POLAR_SUBSCRIPTION_PRODUCT_ID`, `POLAR_SERVER` |
| Reconciliation | `CRON_SECRET` |
| Optional Google login | `NEXT_PUBLIC_GOOGLE_AUTH_ENABLED`, `SUPABASE_AUTH_EXTERNAL_GOOGLE_CLIENT_ID`, `SUPABASE_AUTH_EXTERNAL_GOOGLE_SECRET` |

`NODE_ENV` is managed by Next.js. Public variables are embedded during build;
rebuild after changing them. Server secrets must never use a `NEXT_PUBLIC_` prefix.

## Production setup and launch

1. Provision Supabase, Upstash, AssemblyAI, Turnstile, and Polar. Populate every
   required variable above in the hosting environment. Use HTTPS origins, real
   Turnstile keys, and independent random webhook/cron secrets (`openssl rand -hex 32`).
2. Link the intended Supabase project and apply migrations **before** deploying code:
   `npx supabase link --project-ref YOUR_PROJECT_REF`, then `npx supabase db push`.
   Migration `0007_job_integrity.sql` is required for the new transactional RPCs.
   Set the Supabase storage service's global upload limit to at least 50 MiB;
   the private `audio` bucket also enforces 50 MiB and accepted MIME types.
3. Configure Supabase Auth's site URL and allow-list
   `https://YOUR_DOMAIN/auth/callback*`. Configure production SMTP for magic links.
   For Google, configure the OAuth provider in Supabase and set
   `NEXT_PUBLIC_GOOGLE_AUTH_ENABLED=true`. Locally, also enable `[auth.external.google]`
   in `supabase/config.toml` and export the two Google variables before starting Supabase.
4. In Polar, create one-time USD products priced at $10, $25, and $50. Set
   `POLAR_TOPUP_PRODUCTS=1000:PRODUCT_UUID,2500:PRODUCT_UUID,5000:PRODUCT_UUID`.
   Create a monthly USD $9 product without trial/metered pricing and set
   `POLAR_SUBSCRIPTION_PRODUCT_ID`. Disable discounts and multiple simultaneous
   subscriptions per customer in the product/organization configuration.
   Configure `/api/webhooks/polar` for `order.paid` and `subscription.*` events.
   Use sandbox credentials and `POLAR_SERVER=sandbox` to test first, then replace
   them with production credentials and `POLAR_SERVER=production`.
5. AssemblyAI receives its webhook URL and secret on submission. The public app
   origin must be reachable at `/api/webhooks/assemblyai`. Use Universal-2 support
   and pricing appropriate to the configured trial budget.
6. Schedule `GET /api/cron/reconcile` every five minutes with
   `Authorization: Bearer CRON_SECRET`. `vercel.json` supplies the schedule on
   compatible Vercel plans; other hosts require an external scheduler.
7. Run `npm run check:env`, `npm run lint`, `npm run typecheck`, `npm test`,
   `npm audit`, and **`npm run build`**. Start with **`npm start`** (or deploy on Vercel).
8. Before accepting payments, verify a real short transcription, magic-link/OAuth
   login, a Polar sandbox top-up, subscription activation/renewal/cancellation,
   duplicate webhook delivery, and cron recovery. Then verify the production
   webhook endpoints and a controlled live purchase with the operator's approval.

## Runtime behavior and limits

- Files: non-empty, at most 50 MiB; audio formats listed in `lib/jobs/media.ts`.
  Transcription is capped at two hours, ten minutes for anonymous trials, or the
  duration funded by the account. The result displays when a cap was reached.
- Credit charges round up to whole minutes at $0.05/min. Subscription: $9/month,
  300 minutes. One processing job per account prevents simultaneous balance reuse.
- Job start claims and transcript/debit/plan updates use service-only database
  transactions. Duplicate completion webhooks do not debit twice or overwrite edits.
- Trials reserve three cents against a $5 daily ceiling before processing. Failed or
  abandoned trials retain their reservation and visitor claim, conservatively.
- A random HttpOnly cookie controls anonymous transcript access. Signing in attaches
  completed trial jobs to the account; it never retroactively charges for a trial.
- History is paginated. Transcript edits require ownership and a version match.
  Deletion removes local audio/transcript/history; financial ledger rows remain.
  The transcription provider's separate retention policy still applies.
- Reconciliation retries unfinished provider jobs and expires abandoned uploads.
  Ambiguous submissions are not automatically resubmitted; callbacks can attach the
  provider ID through the signed webhook's job reference. Missing-ID jobs time out
  after six hours. Provider jobs that remain pending require operator investigation.
- Polar refunds/disputes require operator reconciliation of credit; they are not
  automatically deducted. Use the ledger's order ID to identify affected credit.

## Project map

- `app/[locale]`: landing, account, pricing, login, transcript, error/loading pages.
- `app/api`: upload/start/read/edit/delete/export, checkout/portal, webhooks, cron.
- `lib/billing`: prices, order credit validation, subscription synchronization.
- `lib/transcription`: provider interface, AssemblyAI adapter, local fake.
- `lib/quota`: Turnstile, anonymous identity, atomic Redis reservations.
- `supabase/migrations`: schema, ownership policies, grants, transactional billing.
- `scripts`: environment validation and local database/end-to-end checks.

See [docs/AUDIT.md](docs/AUDIT.md) for the verification record and launch limitations.
