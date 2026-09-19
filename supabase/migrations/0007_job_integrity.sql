-- All money and job state transitions are transactional and service-role only.
alter table jobs add column max_duration_seconds integer;
alter table jobs add column reserved_plan_minutes integer not null default 0;
alter table jobs add column rate_cents integer not null default 5;
alter table jobs add column billing_period_start timestamptz;
alter table accounts add column subscription_id text;
alter table accounts add column subscription_updated_at timestamptz;
alter table accounts add column period_end timestamptz;
create index jobs_pending_idx on jobs (status, created_at) where status in ('processing', 'awaiting_upload');

-- Increment under the account row lock. Recomputing SUM in concurrent insert
-- snapshots can lose another transaction's credit.
create or replace function refresh_balance() returns trigger language plpgsql
set search_path = public as $$
begin
  update accounts set balance_cents = balance_cents + new.delta_cents where user_id = new.user_id;
  return new;
end;
$$;

create function reserve_job(p_job_id uuid, p_rate integer, p_max_seconds integer)
returns jsonb language plpgsql set search_path = public as $$
declare j jobs; a accounts; available integer; cap integer;
begin
  if p_rate <= 0 or p_max_seconds <= 0 then raise exception 'invalid limits'; end if;
  select * into j from jobs where id = p_job_id for update;
  if not found then return jsonb_build_object('error', 'not_found'); end if;
  if j.status <> 'awaiting_upload' then return jsonb_build_object('already_started', true); end if;
  cap := p_max_seconds;
  available := 0;
  if j.user_id is not null then
    select * into strict a from accounts where user_id = j.user_id for update;
    -- One processing job per account prevents spending the same balance twice.
    if exists(select 1 from jobs where user_id = j.user_id and status = 'processing') then
      return jsonb_build_object('error', 'job_in_progress');
    end if;
    available := case when a.period_end > now() and a.subscription_status = 'active'
      then greatest(0, a.minutes_included - a.minutes_used) else 0 end;
    cap := least(cap, (available + greatest(0, a.balance_cents) / p_rate) * 60);
    if cap <= 0 then return jsonb_build_object('error', 'insufficient_balance'); end if;
  end if;
  update jobs set status = 'processing', max_duration_seconds = cap,
    reserved_plan_minutes = least(available, ceil(cap / 60.0)::integer),
    billing_period_start = a.period_start, rate_cents = p_rate where id = j.id;
  return jsonb_build_object('maxDurationSeconds', cap);
end;
$$;

create function complete_job(p_provider_id text, p_result jsonb)
returns boolean language plpgsql set search_path = public as $$
declare j jobs; a accounts; seconds integer; minutes integer; plan_minutes integer; cost integer;
begin
  select * into j from jobs where provider_job_id = p_provider_id for update;
  if not found or j.status <> 'processing' then return false; end if;
  if p_result->>'status' = 'failed' then
    update jobs set status = 'failed', error = p_result->>'error', cost_cents = 0, completed_at = now() where id = j.id;
    return true;
  end if;
  if p_result->>'status' <> 'done' then raise exception 'invalid completion'; end if;
  seconds := ceil((p_result->>'durationSeconds')::numeric)::integer;
  if seconds < 0 then raise exception 'invalid duration'; end if;
  minutes := ceil(least(seconds, coalesce(j.max_duration_seconds, seconds)) / 60.0)::integer;
  cost := 0;
  if j.user_id is not null then
    select * into strict a from accounts where user_id = j.user_id for update;
    plan_minutes := least(minutes, j.reserved_plan_minutes);
    cost := (minutes - plan_minutes) * j.rate_cents;
    insert into ledger(user_id, delta_cents, reason, job_id, external_id)
      values(j.user_id, -cost, 'transcription', j.id, 'job:' || j.id || ':debit');
    -- A late completion from a prior billing cycle must not consume the new cycle.
    if a.period_start is not distinct from j.billing_period_start then
      update accounts set minutes_used = minutes_used + plan_minutes where user_id = j.user_id;
    end if;
  end if;
  insert into transcripts(job_id, text, segments, word_count)
    values(j.id, p_result->>'text', p_result->'segments', (p_result->>'wordCount')::integer)
    on conflict (job_id) do nothing;
  update jobs set status = 'done', duration_seconds = seconds, language = p_result->>'language',
    cost_cents = cost, error = null, completed_at = now(),
    truncated = j.max_duration_seconds is not null and seconds >= j.max_duration_seconds where id = j.id;
  return true;
end;
$$;

create function sync_subscription(p_user_id uuid, p_id text, p_status text,
  p_start timestamptz, p_end timestamptz, p_updated timestamptz, p_minutes integer)
returns void language plpgsql set search_path = public as $$
declare a accounts;
begin
  select * into strict a from accounts where user_id = p_user_id for update;
  if a.subscription_updated_at is not null and a.subscription_updated_at >= p_updated then return; end if;
  update accounts set subscription_id = p_id, subscription_status = p_status,
    subscription_updated_at = p_updated, plan = case when p_status = 'active' then 'subscription' else 'payg' end,
    minutes_included = case when p_status = 'active' then p_minutes else 0 end,
    minutes_used = case when period_start is distinct from p_start then 0 else minutes_used end,
    period_start = p_start, period_end = p_end where user_id = p_user_id;
end;
$$;

-- Existing functions also get locked down; no browser may mutate billing via RPC.
revoke execute on all functions in schema public from public, anon, authenticated;
grant execute on all functions in schema public to service_role;
alter default privileges in schema public revoke execute on functions from public;

-- The hosted bucket and browser must enforce the same 50 MiB limit.
update storage.buckets set file_size_limit = 52428800,
  allowed_mime_types = array['audio/mpeg','audio/mp3','audio/wav','audio/x-wav','audio/m4a','audio/x-m4a','audio/mp4','audio/aac','audio/ogg','audio/webm','video/mp4']
  where id = 'audio';

alter table transcripts add column updated_at timestamptz not null default now();

-- Repair accounts for users created before the bootstrap trigger existed.
insert into accounts(user_id) select id from auth.users on conflict do nothing;
