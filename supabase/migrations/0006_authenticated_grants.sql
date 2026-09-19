-- RLS policies decide *which rows* a role may see; they cannot grant the
-- privilege to touch the table in the first place. 0001 added the policies and
-- 0005 granted `service_role`, so every read so far worked only because it went
-- through the service client. The first genuine RLS read (the dashboard,
-- querying as the signed-in user) got `42501 permission denied for table
-- accounts` and rendered an empty account instead.
--
-- SELECT only, and only on the four tables whose policies already scope rows to
-- auth.uid(). Writes stay service-role: there are no INSERT/UPDATE policies.
grant select on public.accounts    to authenticated;
grant select on public.jobs        to authenticated;
grant select on public.transcripts to authenticated;
grant select on public.ledger      to authenticated;
