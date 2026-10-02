import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import hooks from 'eslint-plugin-react-hooks';
import refresh from 'eslint-plugin-react-refresh';
import globals from 'globals';

export default tseslint.config(
  { ignores: ['dist/**', 'node_modules/**', 'src/generated/**'] },
  { files: ['**/*.{js,mjs}'], ...js.configs.recommended,
    languageOptions: { globals: { ...globals.browser, ...globals.node } } },
  {
    files: ['src/**/*.{ts,tsx}', 'vite.config.ts'],
    extends: [js.configs.recommended, ...tseslint.configs.recommendedTypeChecked],
    languageOptions: { parserOptions: { project: './tsconfig.json', tsconfigRootDir: import.meta.dirname } },
    plugins: { 'react-hooks': hooks, 'react-refresh': refresh },
    rules: {
      ...hooks.configs.recommended.rules,
      '@typescript-eslint/no-floating-promises': ['error', { ignoreVoid: false }],
      '@typescript-eslint/no-misused-promises': ['error', { checksVoidReturn: { attributes: false } }],
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }],
    },
  },
);
