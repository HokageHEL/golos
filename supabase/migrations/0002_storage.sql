insert into storage.buckets (id, name, public)
values ('audio', 'audio', false)
on conflict (id) do nothing;
