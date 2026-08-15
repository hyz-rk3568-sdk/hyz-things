import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import { chromium } from 'playwright';

const origins = process.argv.slice(2);
const initialPassword = process.env.HYZ_ROUTER_ADMIN_PASSWORD;
const replacementPassword = process.env.HYZ_ROUTER_REPLACEMENT_PASSWORD;
const bootstrapFirstOrigin = process.env.HYZ_ROUTER_BOOTSTRAP === '1';
const screenshotDirectory = process.env.HYZ_CAMERA_SCREENSHOT_DIR;

if (origins.length === 0 || origins.some(origin => !/^http:\/\/[^/]+(?::\d+)?$/.test(origin))) {
  throw new Error('usage: node e2e/camera-hardware.mjs http://LAN_ORIGIN [http://TAILSCALE_ORIGIN]');
}
if (!initialPassword) {
  throw new Error('HYZ_ROUTER_ADMIN_PASSWORD is required');
}
if (bootstrapFirstOrigin && !replacementPassword) {
  throw new Error('HYZ_ROUTER_REPLACEMENT_PASSWORD is required when HYZ_ROUTER_BOOTSTRAP=1');
}
if (screenshotDirectory) {
  await fs.mkdir(screenshotDirectory, { recursive: true });
}

async function login(page, origin, password, bootstrap) {
  await page.goto(origin, { waitUntil: 'networkidle', timeout: 30_000 });
  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  await page.getByRole('button', { name: '管理员登录' }).click();
  await page.getByLabel('密码').fill(password);
  await page.getByRole('button', { name: '登录', exact: true }).click();
  if (bootstrap) {
    await page.getByLabel('当前密码').fill(password);
    await page.getByLabel('新密码', { exact: true }).fill(replacementPassword);
    await page.getByLabel('确认新密码').fill(replacementPassword);
    await page.getByRole('button', { name: '修改密码' }).click();
  }
  await page.getByRole('heading', { name: '代理设置' }).waitFor({
    state: 'visible',
    timeout: 30_000,
  });
}

async function cameraStatus(page) {
  return page.evaluate(async () => {
    const response = await fetch('/api/v1/camera/status', { cache: 'no-store' });
    if (!response.ok) throw new Error(`camera status HTTP ${response.status}`);
    return response.json();
  });
}

async function sampleVideoPixels(page) {
  return page.evaluate(() => {
    const video = document.querySelector('video[aria-label="摄像头实时画面"]');
    if (!(video instanceof HTMLVideoElement) || video.videoWidth === 0 || video.videoHeight === 0) {
      throw new Error('camera video is not ready for pixel sampling');
    }
    const canvas = document.createElement('canvas');
    canvas.width = 160;
    canvas.height = 90;
    const context = canvas.getContext('2d', { willReadFrequently: true });
    if (!context) throw new Error('2D canvas is unavailable');
    context.drawImage(video, 0, 0, canvas.width, canvas.height);
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
    let sum = 0;
    let sumSquares = 0;
    let brightPixels = 0;
    let maxLuma = 0;
    const count = pixels.length / 4;
    for (let index = 0; index < pixels.length; index += 4) {
      const luma = (pixels[index] * 54 + pixels[index + 1] * 183 + pixels[index + 2] * 19) / 256;
      sum += luma;
      sumSquares += luma * luma;
      if (luma >= 16) brightPixels += 1;
      if (luma > maxLuma) maxLuma = luma;
    }
    const meanLuma = sum / count;
    const variance = Math.max(0, sumSquares / count - meanLuma * meanLuma);
    return {
      meanLuma,
      standardDeviation: Math.sqrt(variance),
      brightPixelRatio: brightPixels / count,
      maxLuma,
    };
  });
}

async function verifyStream(page, originName, viewport) {
  await page.setViewportSize(viewport);
  const camera = page.getByRole('article', { name: '摄像头直播' });
  await camera.waitFor({ state: 'visible', timeout: 30_000 });
  await camera.getByText('可用', { exact: true }).waitFor({ state: 'visible' });
  await camera.getByText(/3840 × 2160 · 30 fps · h264/).waitFor({ state: 'visible' });

  const initial = await cameraStatus(page);
  assert.equal(initial.camera.available, true);
  assert.equal(initial.camera.pipeline, 'stopped');
  assert.equal(initial.camera.active_sessions, 0);

  await camera.getByRole('button', { name: '播放直播' }).click();
  await camera.getByText('直播中', { exact: true }).waitFor({
    state: 'visible',
    timeout: 45_000,
  });

  await page.waitForFunction(() => {
    const video = document.querySelector('video[aria-label="摄像头实时画面"]');
    const stream = video?.srcObject;
    return video instanceof HTMLVideoElement && stream instanceof MediaStream
      && stream.getVideoTracks().some(track => track.readyState === 'live')
      && video.videoWidth > 0 && video.videoHeight > 0;
  }, null, { timeout: 45_000 });

  await page.waitForFunction(async () => {
    const peer = window.__hyzRealPeers?.at(-1);
    if (!peer || peer.connectionState !== 'connected') return false;
    const stats = await peer.getStats();
    for (const report of stats.values()) {
      if (report.type === 'inbound-rtp' && report.kind === 'video'
        && report.bytesReceived > 0 && report.packetsReceived > 0
        && (report.framesDecoded || 0) > 0) return true;
    }
    return false;
  }, null, { timeout: 45_000 });

  const active = await cameraStatus(page);
  assert.equal(active.camera.pipeline, 'streaming');
  assert.equal(active.camera.active_sessions, 1);

  const media = await camera.locator('video').evaluate(video => ({
    width: video.videoWidth,
    height: video.videoHeight,
    readyState: video.readyState,
    paused: video.paused,
  }));
  assert.equal(media.width, 3840);
  assert.equal(media.height, 2160);
  assert.ok(media.readyState >= 2);
  assert.equal(media.paused, false);

  const visual = await sampleVideoPixels(page);
  assert.ok(visual.maxLuma >= 24, `camera frame is black: ${JSON.stringify(visual)}`);
  assert.ok(visual.meanLuma >= 3, `camera frame is too dark: ${JSON.stringify(visual)}`);
  assert.ok(
    visual.standardDeviation >= 2,
    `camera frame has no visible detail: ${JSON.stringify(visual)}`,
  );
  assert.ok(
    visual.brightPixelRatio >= 0.01,
    `camera frame has insufficient non-black pixels: ${JSON.stringify(visual)}`,
  );

  const layout = await page.evaluate(() => ({
    clientWidth: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }));
  assert.ok(layout.scrollWidth <= layout.clientWidth);

  const rotateButton = camera.getByRole('button', { name: '旋转画面' });
  await rotateButton.waitFor({ state: 'visible' });
  let rotation = 0;
  for (const deg of [90, 180, 270, 0]) {
    await rotateButton.click();
    const className = await camera.locator('video').evaluate(video => video.className);
    assert.ok(
      className.includes(`rotate-${deg}`),
      `rotation ${deg}deg applied (className=${className})`,
    );
    rotation = deg;
  }

  if (screenshotDirectory) {
    await page.screenshot({
      path: path.join(screenshotDirectory, `camera-${originName}-${viewport.width}.png`),
      fullPage: true,
    });
  }

  await camera.getByRole('button', { name: '停止直播' }).click();
  await camera.getByText('未播放', { exact: true }).waitFor({
    state: 'visible',
    timeout: 30_000,
  });
  await page.waitForFunction(async () => {
    const response = await fetch('/api/v1/camera/status', { cache: 'no-store' });
    if (!response.ok) return false;
    const body = await response.json();
    return body.camera.pipeline === 'stopped' && body.camera.active_sessions === 0;
  }, null, { timeout: 30_000 });

  return { media, visual, layout, rotation };
}

const browser = await chromium.launch({
  headless: true,
  args: [`--unsafely-treat-insecure-origin-as-secure=${origins.join(',')}`],
});
const results = [];
let password = initialPassword;
try {
  for (const [index, origin] of origins.entries()) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    await context.addInitScript(() => {
      const NativePeerConnection = window.RTCPeerConnection;
      const peers = [];
      window.__hyzRealPeers = peers;
      window.RTCPeerConnection = class extends NativePeerConnection {
        constructor(...args) {
          super(...args);
          peers.push(this);
        }
      };
    });
    const page = await context.newPage();
    const browserErrors = [];
    page.on('pageerror', error => browserErrors.push(`pageerror: ${error.message}`));
    page.on('console', message => {
      if (message.type() === 'error') browserErrors.push(`console: ${message.text()}`);
    });
    try {
      const bootstrap = bootstrapFirstOrigin && index === 0;
      await login(page, origin, password, bootstrap);
      if (bootstrap) password = replacementPassword;
      const originName = new URL(origin).hostname.replaceAll(':', '_');
      const desktop = await verifyStream(page, originName, { width: 1280, height: 800 });
      const mobile = await verifyStream(page, originName, { width: 360, height: 800 });
      assert.deepEqual(browserErrors, []);
      results.push({ origin, desktop, mobile });
    } finally {
      await context.close();
    }
  }
} finally {
  await browser.close();
}

console.log(JSON.stringify({ ok: true, results }, null, 2));
