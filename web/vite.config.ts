import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  cacheDir: 'node_modules/.vite',
  build: { sourcemap: false, target: 'es2023', assetsInlineLimit: 0 },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
    maxWorkers: 2,
    restoreMocks: true,
    clearMocks: true,
  },
});
