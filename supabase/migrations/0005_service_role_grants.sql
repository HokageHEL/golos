-- Every server-side write in this app goes through the service-role client:
-- the webhook handler, the reconcile cron, the anonymous result page.
-- Verified empirically: after `supabase db reset`, service_role holds ZERO
-- INSERT privileges on public tables, so those writes fail. Hosted Supabase
-- grants these by default; making it explicit means local and hosted behave
-- the same instead of the difference surfacing as a mystery in production.
grant usage on schema public to service_role;
grant all privileges on all tables in schema public to service_role;
grant all privileges on all sequences in schema public to service_role;

-- Tables added by later migrations inherit the same grants.
alter default privileges in schema public
  grant all privileges on tables to service_role;
alter default privileges in schema public
  grant all privileges on sequences to service_role;
