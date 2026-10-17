import { defineConfig, globalIgnores } from 'eslint/config';
import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';

export default defineConfig([
  globalIgnores(['native/dist/**', 'native/src-tauri/target/**', 'native/src-tauri/gen/**']),
  js.configs.recommended,
  tseslint.configs.recommended,
  {
    files: ['native/src/**/*.{ts,tsx}'],
    plugins: { 'react-hooks': reactHooks },
    rules: reactHooks.configs.recommended.rules,
  },
]);
