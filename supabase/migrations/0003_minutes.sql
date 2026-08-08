create function add_minutes_used(p_user_id uuid, p_minutes integer)
returns void language sql as $$
  update accounts set minutes_used = minutes_used + p_minutes where user_id = p_user_id;
$$;
