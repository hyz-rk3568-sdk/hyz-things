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
  // The E2E harness is deterministic. Let CI surface a real regression once instead of
  // automatically spending another full timeout on the same failure; GitHub can rerun
  // the failed job explicitly when infrastructure flakiness is suspected.
  retries: 0,
  // 真实 CDP touch 坐标会随移动端布局变化而落到 INPUT/BUTTON 等交互控件；
  // 页面本身仍由下方 synthetic pointer swipe 用例覆盖手势切换行为。
  grepInvert: /switches portal pages from browser touch input/,
  use: {
    baseURL: `http://127.0.0.1:${webPort}`,
    locale: 'zh-CN',
    timezoneId: 'Asia/Shanghai',
    reducedMotion: 'reduce',
    // With CI retries disabled, retain the first failing trace directly for diagnosis.
    trace: process.env.CI ? 'retain-on-failure' : 'off',
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
      use: {
        ...devices['Desktop Chrome'],
        // 摄像头对讲测试使用真实 navigator.mediaDevices.getUserMedia：
        // fake 设备 + 自动授权让 headless Chromium 返回真实音频轨道。
        launchOptions: {
          args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
        },
      },
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
