-- Ensures every auth.users row has a corresponding accounts row, no matter
-- how the user was created (magic link, dashboard, future OAuth provider).
create function bootstrap_account() returns trigger language plpgsql security definer
set search_path = public
as $$
begin
  insert into accounts (user_id) values (new.id) on conflict do nothing;
  return new;
end;
$$;

create trigger on_auth_user_created
  after insert on auth.users
  for each row execute function bootstrap_account();
