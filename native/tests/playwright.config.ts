import { defineConfig } from '@playwright/test';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

// Parallel worktrees must never reuse another checkout's mocked IPC fixture.
// A stable per-checkout port also keeps manual previews on 1420 independent.
const defaultPort = 20000 + Number.parseInt(createHash('sha256').update(process.cwd()).digest('hex').slice(0, 4), 16) % 20000;
const port = Number(process.env.GOLOS_UI_PORT ?? defaultPort);
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error('GOLOS_UI_PORT must be an integer from 1024 to 65535.');

export default defineConfig({
  testDir: '.',
  testMatch: 'usability.spec.ts',
  outputDir: join(tmpdir(), 'golos-usability-browser'),
  workers: 1,
  use: { baseURL: `http://127.0.0.1:${port}`, viewport: { width: 1180, height: 780 }, screenshot: 'only-on-failure' },
  webServer: { command: `npm run native:dev -- --host 127.0.0.1 --port ${port} --strictPort`, url: `http://127.0.0.1:${port}`, reuseExistingServer: false },
});
