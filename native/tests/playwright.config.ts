import { defineConfig } from '@playwright/test';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export default defineConfig({
  testDir: '.',
  testMatch: 'usability.spec.ts',
  outputDir: join(tmpdir(), 'golos-usability-browser'),
  workers: 1,
  use: { baseURL: 'http://127.0.0.1:1421', viewport: { width: 1180, height: 780 }, screenshot: 'only-on-failure' },
  webServer: { command: 'npm run native:dev -- --port 1421', url: 'http://127.0.0.1:1421', reuseExistingServer: true },
});
