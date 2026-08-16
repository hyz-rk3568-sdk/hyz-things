import { defineConfig, devices } from '@playwright/test';

const defaultWebPort = 20_000 + (process.pid % 20_000) * 2;
const webPort = parsePort(process.env.HYZ_THINGS_E2E_WEB_PORT, defaultWebPort, 'HYZ_THINGS_E2E_WEB_PORT');
const controlPort = parsePort(
  process.env.HYZ_THINGS_E2E_CONTROL_PORT,
  webPort + 1,
  'HYZ_THINGS_E2E_CONTROL_PORT',
);

process.env.HYZ_THINGS_E2E_WEB_PORT = String(webPort);
process.env.HYZ_THINGS_E2E_CONTROL_PORT = String(controlPort);

export default defineConfig({
  testDir: './e2e',
  workers: 1,
  fullyParallel: false,
  timeout: 30_000,
  expect: {
    timeout: 7_500,
  },
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL: `http://127.0.0.1:${webPort}`,
    locale: 'zh-CN',
    timezoneId: 'Asia/Shanghai',
    reducedMotion: 'reduce',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  webServer: {
    command: 'bash tools/start-e2e-server.sh',
    url: `http://127.0.0.1:${webPort}/api/v1/health`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});

function parsePort(value: string | undefined, fallback: number, name: string): number {
  const port = value === undefined ? fallback : Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new Error(`${name} must be an integer from 1 through 65535`);
  }
  return port;
}
