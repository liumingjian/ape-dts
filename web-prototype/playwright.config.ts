import { defineConfig, devices } from '@playwright/test';

// Inherited HTTP_PROXY env vars (e.g. corporate proxies on 127.0.0.1) break
// loopback connections from chromium. Pin no_proxy here so both the webServer
// probe and the browser bypass the proxy for the local dev server.
process.env.NO_PROXY = '127.0.0.1,localhost,::1';
process.env.no_proxy = '127.0.0.1,localhost,::1';
delete process.env.HTTP_PROXY;
delete process.env.HTTPS_PROXY;
delete process.env.http_proxy;
delete process.env.https_proxy;

// E2E_REAL_BACKEND=1 → connect to already-running Vite on :5173 with
// VITE_USE_MOCK=false. Used for full-happy-path tests that require the
// real dt-console-server + docker stack. Default (unset) → MSW mode on :4173.
const REAL_BACKEND = !!process.env.E2E_REAL_BACKEND;
const PORT = Number(process.env.E2E_PORT ?? (REAL_BACKEND ? 5173 : 4173));
const baseURL = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  testDir: './e2e',
  timeout: REAL_BACKEND ? 120_000 : 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: process.env.CI ? [['github'], ['list']] : 'list',
  use: {
    baseURL,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'off',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
  webServer: REAL_BACKEND
    ? {
        // Real-backend mode: connect to an already-running dev server.
        // The test runner will NOT start its own server.
        url: baseURL,
        reuseExistingServer: true,
      }
    : {
        // Default MSW mode: start a preview server with mock service worker.
        command: `pnpm exec vite --host 127.0.0.1 --port ${PORT} --strictPort`,
        url: baseURL,
        reuseExistingServer: !process.env.CI,
        timeout: 120_000,
        stdout: 'pipe',
        stderr: 'pipe',
        env: { VITE_USE_MOCK: 'true' },
      },
});
