# Codebase audit and handoff — 2026-09-19

## Scope and preserved work

Reviewed application routes, components, translation catalogs, billing/transcription/
quota modules, exports, tests, dependency manifests/lockfile, Next/TypeScript/ESLint/
Vitest settings, Supabase configuration and migrations, Docker/Vercel configuration,
environment handling, and the existing design/implementation documents. Existing
uncommitted user work was retained. No hosted deployment or real purchase was made.

## Feature map and fixes

| Area | Finding | Result |
| --- | --- | --- |
| Upload | Direct storage uploads existed; malformed requests, empty/oversized files, network failures, and duplicate clicks were weakly handled | Browser/API/bucket limits, validation, timeouts, pending states, localized errors, explicit Turnstile lifecycle, interrupted-start recovery |
| Transcription | Concurrent starts could submit twice; pending provider jobs were treated as complete | Database start claim, one processing job per account, funded duration caps, pending-state handling, plain-text fallback when utterances are absent |
| Billing | Ledger, plan consumption, transcript, and status were separate writes; balance cache could lose concurrent credits | Transactional service-only RPCs, account/job locks, incremental balance trigger, rate/plan snapshot, replay-safe completion |
| Anonymous trial | Predictable access token; budget counted after work and could overshoot | Random HttpOnly access cookie, atomic Redis reservation before work, failure-closed bot/quota handling |
| Account conversion | Trial transcript stayed anonymous after login | Completed trials attach to the account at callback/dashboard, without retroactive charging |
| Editing | `contentEditable` changes disappeared on refresh and did not affect exports | Explicit edit/save/preview/copy, version conflicts, unsaved navigation protection, saved exports, visible timestamps, word-count updates |
| Export | Existing TXT/DOCX/SRT encoders and filename handling | Format validation, authenticated ownership, download feedback and recoverable errors |
| History | No result links, pagination, deletion, sign-out, or payment refresh | All added, with terminal-job deletion and bounded background refresh |
| Subscriptions | Disabled checkout button | Configured Polar subscription checkout, current-state synchronization, monthly entitlement updates, management portal |
| Payments | Top-up implementation existed | Configured-product allow-list, duplicate-order handling, signed-event failure responses, regression coverage |
| Authentication | Session refresh missing from middleware; Google shown without setup | Next.js `proxy.ts`, refreshed cookies forwarded to server rendering, localized callback, optional Google button, pending/errors |
| UI and accessibility | Blank loading state, missing page error boundary, weak narrow-screen behavior | Localized loading/error/404/empty states, progress semantics, responsive nav/history wrapping, reduced-motion toast fix |
| Operations | Abandoned uploads and completion recovery existed with races | Atomic reaping claim, recoverable completion module, webhook job-ID recovery, documented scheduler/limits |
| Dependencies | Critical Next.js and additional vulnerable dependencies | Next.js/eslint-config-next 16.3.5, patched Vitest/YAML dependencies, zero audit findings |
| Environment | Partial setup documentation; Google/local-provider assumptions | Complete commented `.env.example`, secret-safe production validator, revised local/production instructions |

## Verification performed

- `npm run lint`: passed, zero warnings.
- `npm run typecheck`: passed.
- `npm test`: **112 tests passed in 22 files**.
- `npm run test:db`: passed against local Supabase; all seven migrations applied.
  Test fixtures roll back. Checks include credit caps, start idempotency, overlapping
  account work, atomic debit/transcript completion, subscription replay/order handling,
  RPC execution permissions, and cross-account RLS.
- `npm run test:smoke -- http://localhost:3100`: passed with a temporary local account,
  synthetic WAV, real local storage, real Redis, Cloudflare's public test keys, and
  the development fake transcription provider. Covered paid/anonymous paths, simultaneous
  start requests, webhook completion, single debit, ownership denial, edit conflicts,
  all three exports, deletion, trial reuse denial, and account claiming.
- `npm run build`: passed with Next.js 16.3.5, including TypeScript and route generation.
- Built server (`npm start -- --hostname localhost --port 3101`): English landing,
  Ukrainian landing, pricing, and login returned 200; unknown page returned 404.
- `npm audit`: **0 vulnerabilities**, including development dependencies.
- `git diff --check`: passed. Environment-reference inventory: no undocumented app variables.
- Browser rendering tests verify translated controls and transcript structure. Static
  CSS review covered mobile wrapping and reduced-motion behavior. Native browser visual
  QA was attempted but blocked by the computer-use service (`cgWindowNotFound`);
  **real mobile layout, interaction, and browser-console verification remain unverified**.

The test server exposed an upstream Next.js loopback rewrite issue when bound to
`127.0.0.1`. Binding it to `localhost` restored normal routes. Use the default hostname
or `localhost`; [upstream issue](https://github.com/vercel/next.js/issues/94745).

## Launch requirements and limitations

The code builds and the local fake-provider flow works end to end. It is **not a
verified live deployment**. The current environment intentionally fails `npm run
check:env`: real AssemblyAI/payment configuration and production origins/secrets are
missing or still local/test values. `.env.local` was not changed.

Before launch, the operator must:

1. Fill the variables listed in `.env.example`; apply `0007_job_integrity.sql` to the
   hosted database before deploying this code. `README.md` contains exact commands.
2. Configure SMTP/OAuth redirects, Turnstile domains, Polar products/webhooks/portal,
   and a working five-minute cron. Google is optional and disabled locally by default.
3. Verify real AssemblyAI transcription and Polar sandbox purchase, renewal,
   cancellation, and repeated webhook delivery. These external flows could not be
   live-tested without credentials. No real money was spent during verification.
4. Complete desktop/mobile visual and browser-console checks on a working browser.
5. Account for the documented operational policies: 50 MiB/two-hour file cap,
   funded-duration truncation, one active transcription per account, conservative
   trial reservations, provider retention, and manual refund/dispute reconciliation.

Original sample assets and baseline pure unit-test helpers remain harmless. No new
cloud resources, production secrets, remote database changes, or deployments were made.

## Environment inventory

Required for the complete paid production service:

```text
NEXT_PUBLIC_SUPABASE_URL
NEXT_PUBLIC_SUPABASE_ANON_KEY
SUPABASE_SERVICE_ROLE_KEY
NEXT_PUBLIC_APP_URL
ASSEMBLYAI_API_KEY
ASSEMBLYAI_WEBHOOK_SECRET
UPSTASH_REDIS_REST_URL
UPSTASH_REDIS_REST_TOKEN
NEXT_PUBLIC_TURNSTILE_SITE_KEY
TURNSTILE_SECRET_KEY
POLAR_ACCESS_TOKEN
POLAR_WEBHOOK_SECRET
POLAR_TOPUP_PRODUCTS
POLAR_SUBSCRIPTION_PRODUCT_ID
POLAR_SERVER
CRON_SECRET
```

Optional Google login:

```text
NEXT_PUBLIC_GOOGLE_AUTH_ENABLED
SUPABASE_AUTH_EXTERNAL_GOOGLE_CLIENT_ID
SUPABASE_AUTH_EXTERNAL_GOOGLE_SECRET
```

`NODE_ENV` is supplied by Next.js. Google CLI variables belong in the shell for local
Supabase; hosted Google credentials belong in the Supabase provider configuration.
