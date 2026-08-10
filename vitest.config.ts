import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tsconfigPaths from 'vite-tsconfig-paths';

export default defineConfig({
  plugins: [react(), tsconfigPaths()],
  // `next` publishes no exports map for these, so outside the Next bundler
  // they only resolve with the extension. Without this, importing any
  // component that navigates fails to load at all.
  resolve: { alias: { 'next/navigation': 'next/navigation.js' } },
  test: {
    environment: 'node',
    include: ['**/*.test.ts', '**/*.test.tsx'],
    // next-intl must go through Vite for the alias above to reach it;
    // externalised, Node resolves its imports itself and ignores the alias.
    server: { deps: { inline: ['next-intl'] } },
  },
});
