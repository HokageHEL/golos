import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const sql = readFileSync(new URL('./database-check.sql', import.meta.url), 'utf8');
const result = spawnSync('docker', ['exec', '-i', 'supabase_db_golos', 'psql', '-U', 'postgres', '-d', 'postgres'], { input: sql, encoding: 'utf8' });
process.stdout.write(result.stdout || '');
process.stderr.write(result.stderr || '');
if (result.error) console.error(result.error.message);
process.exitCode = result.status ?? 1;
