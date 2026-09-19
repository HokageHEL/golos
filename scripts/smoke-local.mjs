import nextEnv from '@next/env';
const { loadEnvConfig } = nextEnv;
import { createClient } from '@supabase/supabase-js';
import { createServerClient } from '@supabase/ssr';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
loadEnvConfig(process.cwd());
const origin = process.argv[2] || 'http://localhost:3000';
const dbUrl = process.env.NEXT_PUBLIC_SUPABASE_URL;
if (![origin, dbUrl].every((url) => ['localhost', '127.0.0.1'].includes(new URL(url).hostname))) throw new Error('Smoke tests require local app and database');
if (process.env.ASSEMBLYAI_API_KEY) throw new Error('Smoke tests require the free local fake provider');
const db = createClient(dbUrl, process.env.SUPABASE_SERVICE_ROLE_KEY, { auth: { persistSession: false } });
const cookieJar = new Map();
const session = createServerClient(dbUrl, process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY, { cookies: {
  getAll: () => [...cookieJar].map(([name,value]) => ({ name,value })),
  setAll: (values) => values.forEach(({ name, value }) => cookieJar.set(name,value)),
} });
const email = `golos-smoke-${randomUUID()}@example.test`;
const password = randomUUID();
let userId;
let jobId;
async function api(path, options = {}, cookies = '') {
  const res = await fetch(`${origin}${path}`, { ...options, headers: {
    'content-type': 'application/json', origin, cookie: cookies, ...options.headers,
  }, redirect: 'manual' });
  return res;
}
try {
  const { data, error } = await db.auth.admin.createUser({ email, password, email_confirm: true });
  if (error) throw error; userId = data.user.id;
  const { error: creditError } = await db.from('ledger').insert({ user_id: userId, delta_cents: 100, reason: 'local_smoke', external_id: randomUUID() });
  if (creditError) throw creditError;
  const { error: loginError } = await session.auth.signInWithPassword({ email, password });
  if (loginError) throw loginError;
  const cookies = [...cookieJar].map(([name,value]) => `${name}=${encodeURIComponent(value)}`).join('; ');
  assert.equal((await api('/api/jobs/init', { method: 'POST', body: '{}' }, cookies)).status, 400);
  const init = await api('/api/jobs/init', { method: 'POST', body: JSON.stringify({ filename:'smoke.wav', mimeType:'audio/wav', size:16044, fingerprint:'', turnstileToken:null }) }, cookies);
  assert.equal(init.status, 200, await init.clone().text());
  const job = await init.json(); jobId = job.jobId;
  const wav = Buffer.alloc(16044);
  wav.write('RIFF'); wav.writeUInt32LE(16036,4); wav.write('WAVEfmt ',8); wav.writeUInt32LE(16,16); wav.writeUInt16LE(1,20); wav.writeUInt16LE(1,22); wav.writeUInt32LE(8000,24); wav.writeUInt32LE(16000,28); wav.writeUInt16LE(2,32); wav.writeUInt16LE(16,34); wav.write('data',36); wav.writeUInt32LE(16000,40);
  const upload = await fetch(job.uploadUrl, { method:'PUT', headers:{'content-type':'audio/wav'}, body:wav });
  assert.equal(upload.ok, true, await upload.text());
  const starts = await Promise.all([1,2].map(() => api(`/api/jobs/${jobId}/start`, { method:'POST' }, cookies)));
  for(const res of starts) assert.equal(res.status,200,await res.text());
  let result;
  for(let n=0;n<30;n++) {
    result = await (await api(`/api/jobs/${jobId}`,{},cookies)).json();
    if(result.status==='done') break;
    await new Promise((resolve)=>setTimeout(resolve,1000));
  }
  assert.equal(result.status,'done');
  assert.equal((await api(`/api/jobs/${jobId}`)).status,404);
  assert.equal((await api(`/api/jobs/${jobId}/export`)).status,401);
  const { data: entries } = await db.from('ledger').select('delta_cents').eq('job_id',jobId);
  assert.deepEqual(entries,[{delta_cents:-5}]);
  const edited = [{ startMs:0,endMs:19000,speaker:'A',text:'Saved smoke test text' }];
  const patch = {method:'PATCH',body:JSON.stringify({ segments:edited,updatedAt:result.transcript.updated_at })};
  assert.equal((await api(`/api/jobs/${jobId}`,patch,cookies)).status,200);
  assert.equal((await api(`/api/jobs/${jobId}`,patch,cookies)).status,409);
  for(const format of ['txt','srt','docx']) {
    const res = await api(`/api/jobs/${jobId}/export?format=${format}`,{},cookies);
    assert.equal(res.status,200);
    if(format==='docx') assert.ok((await res.arrayBuffer()).byteLength>1000);
    else assert.match(await res.text(),/Saved smoke test text/);
  }
  assert.equal((await api(`/api/jobs/${jobId}`,{method:'DELETE'},cookies)).status,200);
  assert.equal((await api(`/api/jobs/${jobId}`,{},cookies)).status,404);
  // Exercise anonymous quota, the opaque ownership cookie, and sign-in claiming.
  const anonBody = { filename:'trial.wav', mimeType:'audio/wav', size:wav.length,
    fingerprint:randomUUID(), turnstileToken:'XXXX.DUMMY.TOKEN.XXXX' };
  const anonInit = await api('/api/jobs/init', { method:'POST', body:JSON.stringify(anonBody) });
  assert.equal(anonInit.status,200,await anonInit.clone().text());
  const anonCookie = anonInit.headers.get('set-cookie').split(';')[0];
  const anonJob = await anonInit.json(); jobId=anonJob.jobId;
  assert.equal((await fetch(anonJob.uploadUrl,{method:'PUT',headers:{'content-type':'audio/wav'},body:wav})).ok,true);
  assert.equal((await api(`/api/jobs/${jobId}/start`,{method:'POST'},anonCookie)).status,200);
  for(let n=0;n<30;n++) {
    result=await (await api(`/api/jobs/${jobId}`,{},anonCookie)).json();
    if(result.status==='done') break;
    await new Promise((resolve)=>setTimeout(resolve,1000));
  }
  assert.equal(result.status,'done'); assert.equal(result.canExport,false);
  assert.equal((await api('/api/jobs/init',{method:'POST',body:JSON.stringify(anonBody)})).status,403);
  const dashboard = await api('/uk/dashboard',{},`${cookies}; ${anonCookie}`);
  assert.equal(dashboard.status,200); await dashboard.text();
  const claimed = await (await api(`/api/jobs/${jobId}`,{},cookies)).json();
  assert.equal(claimed.canExport,true);
  assert.equal((await api(`/api/jobs/${jobId}`,{},anonCookie)).status,404);
  assert.equal((await api(`/api/jobs/${jobId}/export`,{},cookies)).status,200);
  assert.equal((await api(`/api/jobs/${jobId}`,{method:'DELETE'},cookies)).status,200);
  console.log('PASS: sign-in, upload, concurrent starts, fake webhook, one debit, ownership, edit conflict, TXT/SRT/DOCX exports, deletion, anonymous quota and account claiming');
} finally {
  if(jobId) {
    await db.storage.from('audio').remove([`${jobId}/source.wav`]);
    await db.from('jobs').delete().eq('id',jobId);
  }
  if(userId) await db.auth.admin.deleteUser(userId);
}
