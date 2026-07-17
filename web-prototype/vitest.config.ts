import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';
import Icons from 'unplugin-icons/vite';
import { fileURLToPath, URL } from 'node:url';

export default defineConfig({
  plugins: [vue(), Icons({ autoInstall: false, compiler: 'vue3' })],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
      '@tests': fileURLToPath(new URL('./tests', import.meta.url)),
    },
  },
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./tests/setup.ts'],
    include: ['tests/**/*.spec.ts'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'lcov'],
      include: ['src/**/*.{ts,vue}', 'tests/stubs/**/*.ts'],
      exclude: [
        'src/main.ts',
        'src/mock/**',
        'src/**/*.d.ts',
        'src/locales/**',
        'src/styles/**',
      ],
    },
  },
});
