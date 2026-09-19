\set ON_ERROR_STOP on
begin;
-- Fixtures always roll back; existing local data is not reset.
do $$
declare u uuid := gen_random_uuid(); other_user uuid := gen_random_uuid(); j uuid := gen_random_uuid(); j2 uuid := gen_random_uuid(); r jsonb; ok boolean; n integer;
begin
  insert into auth.users(id) values (u), (other_user);
  insert into ledger(user_id, delta_cents, reason, external_id) values(u, 25, 'test', gen_random_uuid()::text);
  insert into jobs(id, user_id, filename, storage_path) values(j,u,'test.mp3','test'),(j2,u,'other.mp3','other');
  r := reserve_job(j, 5, 7200);
  if (r->>'maxDurationSeconds')::integer <> 300 then raise exception 'credit cap failed'; end if;
  r := reserve_job(j, 5, 7200);
  if r->>'already_started' <> 'true' then raise exception 'duplicate start allowed'; end if;
  r := reserve_job(j2, 5, 7200);
  if r->>'error' <> 'job_in_progress' then raise exception 'concurrent spending allowed'; end if;
  update jobs set provider_job_id = j::text where id = j;
  ok := complete_job(j::text, '{"status":"done","durationSeconds":61,"text":"hello world","segments":[],"wordCount":2}');
  if not ok then raise exception 'completion failed'; end if;
  if (select balance_cents from accounts where user_id=u) <> 15 then raise exception 'wrong debit'; end if;
  ok := complete_job(j::text, '{"status":"done","durationSeconds":61,"text":"overwrite","segments":[],"wordCount":1}');
  if ok then raise exception 'duplicate completion applied'; end if;
  if (select text from transcripts where job_id=j) <> 'hello world' then raise exception 'replay overwrote transcript'; end if;
  if (select count(*) from ledger where job_id=j) <> 1 then raise exception 'duplicate debit'; end if;
  perform sync_subscription(u, 'test-sub', 'active', now(), now()+interval '1 month', now(), 300);
  r := reserve_job(j2,5,7200);
  if (r->>'maxDurationSeconds')::integer <> 7200 then raise exception 'plan cap failed'; end if;
  update jobs set provider_job_id=j2::text where id=j2;
  perform complete_job(j2::text, '{"status":"done","durationSeconds":120,"text":"plan","segments":[],"wordCount":1}');
  if (select minutes_used from accounts where user_id=u) <> 2 then raise exception 'plan not consumed'; end if;
  if (select balance_cents from accounts where user_id=u) <> 15 then raise exception 'plan charged credit'; end if;
  perform sync_subscription(u, 'test-sub', 'active', now(), now()+interval '1 month', now()+interval '1 second', 300);
  if (select minutes_used from accounts where user_id=u) <> 2 then raise exception 'subscription replay reset minutes'; end if;
  perform sync_subscription(u, 'test-sub', 'canceled', now()-interval '1 month', now(), now()-interval '1 day', 300);
  if (select subscription_status from accounts where user_id=u) <> 'active' then raise exception 'stale subscription overwrote current'; end if;
  if has_function_privilege('authenticated','reserve_job(uuid,integer,integer)','execute') then raise exception 'billing RPC exposed'; end if;
  if has_function_privilege('anon','complete_job(text,jsonb)','execute') then raise exception 'completion RPC exposed'; end if;
  perform set_config('request.jwt.claim.sub', other_user::text, true);
  set local role authenticated;
  select count(*) into n from jobs where id=j;
  if n <> 0 then raise exception 'RLS exposed another account job'; end if;
  reset role;
  raise notice 'PASS: funded limits, duplicate starts, concurrent spending, atomic completion, replay protection, subscription periods, RPC grants, RLS';
end;
$$;
rollback;
