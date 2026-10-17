import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'node',
    include: ['native/src/**/*.test.ts', 'native/src/**/*.test.tsx'],
  },
});
