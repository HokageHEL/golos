create type job_status as enum ('awaiting_upload', 'processing', 'done', 'failed');

create table accounts (
  user_id uuid primary key references auth.users on delete cascade,
  balance_cents integer not null default 0,
  plan text not null default 'payg',
  subscription_status text,
  minutes_included integer not null default 0,
  minutes_used integer not null default 0,
  period_start timestamptz,
  created_at timestamptz not null default now()
);

create table jobs (
  id uuid primary key default gen_random_uuid(),
  user_id uuid references auth.users on delete cascade,
  anon_key text,
  filename text not null,
  storage_path text not null,
  status job_status not null default 'awaiting_upload',
  provider text not null default 'assemblyai',
  provider_job_id text unique,
  duration_seconds integer,
  language text,
  cost_cents integer,
  truncated boolean not null default false,
  error text,
  created_at timestamptz not null default now(),
  completed_at timestamptz,
  constraint jobs_owner_present check (user_id is not null or anon_key is not null)
);

create index jobs_user_created_idx on jobs (user_id, created_at desc);

create table transcripts (
  job_id uuid primary key references jobs on delete cascade,
  text text not null,
  segments jsonb not null,
  word_count integer not null,
  created_at timestamptz not null default now()
);

-- Append-only. Never UPDATE or DELETE rows here.
create table ledger (
  id bigserial primary key,
  user_id uuid not null references auth.users on delete cascade,
  delta_cents integer not null,
  reason text not null,
  job_id uuid references jobs on delete set null,
  external_id text not null unique,
  created_at timestamptz not null default now()
);

create index ledger_user_idx on ledger (user_id);

-- balance_cents is a cache of sum(ledger.delta_cents), maintained here so that
-- no application code is ever trusted to keep it in sync.
create function refresh_balance() returns trigger language plpgsql as $$
begin
  update accounts
     set balance_cents = (select coalesce(sum(delta_cents), 0)
                            from ledger where user_id = new.user_id)
   where user_id = new.user_id;
  return new;
end;
$$;

create trigger ledger_refresh_balance
  after insert on ledger
  for each row execute function refresh_balance();

alter table accounts    enable row level security;
alter table jobs        enable row level security;
alter table transcripts enable row level security;
alter table ledger      enable row level security;

create policy own_account on accounts
  for select using (auth.uid() = user_id);

create policy own_jobs on jobs
  for select using (auth.uid() = user_id);

create policy own_transcripts on transcripts
  for select using (
    exists (select 1 from jobs j where j.id = job_id and j.user_id = auth.uid())
  );

create policy own_ledger on ledger
  for select using (auth.uid() = user_id);
