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
  await camera.getByText(/1280 × 720 · 30 fps · 2\.5 Mbps · h264/).waitFor({ state: 'visible' });

  const presetSelect = camera.getByRole('combobox', { name: '画面分辨率与码率' });
  await presetSelect.waitFor({ state: 'visible' });
  assert.equal(await presetSelect.locator('option').count(), 4);

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
  assert.equal(media.width, 1280);
  assert.equal(media.height, 720);
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

  // Switch to 4K while playing: the session stops and reopens at the higher profile.
  await presetSelect.selectOption('uhd4k20m');
  await camera.getByText('直播中', { exact: true }).waitFor({
    state: 'visible',
    timeout: 45_000,
  });
  await page.waitForFunction(() => {
    const video = document.querySelector('video[aria-label="摄像头实时画面"]');
    return video instanceof HTMLVideoElement && video.videoWidth === 3840
      && video.videoHeight === 2160;
  }, null, { timeout: 45_000 });
  await camera.getByText(/3840 × 2160 · 30 fps · 20\.0 Mbps · h264/).waitFor({
    state: 'visible',
  });

  const layout = await page.evaluate(() => ({
    clientWidth: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }));
  assert.ok(layout.scrollWidth <= layout.clientWidth);

  const rotateButton = camera.getByRole('button', { name: '旋转画面' });
  await rotateButton.waitFor({ state: 'visible' });
  // Rotation is a server-side media-pipeline property: the session stops, the
  // camera reopens the pipeline with videoflip applied before the timestamp
  // watermark, and the browser decodes the rotated stream. The button cycles
  // 0 → 270 → 180 → 90 → 0; 90/270 swap width and height.
  for (const [deg, width, height] of [
    [270, 2160, 3840],
    [180, 3840, 2160],
    [90, 2160, 3840],
    [0, 3840, 2160],
  ]) {
    await rotateButton.waitFor({ state: 'visible' });
    await rotateButton.click();
    await camera.getByText('直播中', { exact: true }).waitFor({
      state: 'visible',
      timeout: 45_000,
    });
    await page.waitForFunction(
      expected => {
        const video = document.querySelector('video[aria-label="摄像头实时画面"]');
        return video instanceof HTMLVideoElement && video.videoWidth === expected.width
          && video.videoHeight === expected.height;
      },
      { width, height },
      { timeout: 45_000 },
    );
    const status = await cameraStatus(page);
    assert.equal(status.camera.profile.rotation, `deg_${deg}`);
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

// 并发观看：同一浏览器上下文（同一管理员 cookie、同一 owner）开第二个页面，
// 两个页面同时观看同一路 720p 编码流；关闭其一不影响另一个。
async function verifyConcurrentViewers(context, origin, originName) {
  const pageA = context.pages()[0];
  const cameraA = pageA.getByRole('article', { name: '摄像头直播' });
  await cameraA.waitFor({ state: 'visible', timeout: 30_000 });

  // verifyStream 结束时停留在 4K 预设：先切回默认 720p，减轻双路解码负载。
  const presetSelect = cameraA.getByRole('combobox', { name: '画面分辨率与码率' });
  await presetSelect.selectOption('hd720p25m');
  await cameraA.getByText(/1280 × 720 · 30 fps · 2\.5 Mbps · h264/).waitFor({
    state: 'visible',
  });

  await cameraA.getByRole('button', { name: '播放直播' }).click();
  await cameraA.getByText('直播中', { exact: true }).waitFor({
    state: 'visible',
    timeout: 45_000,
  });

  const pageB = await context.newPage();
  const browserErrorsB = [];
  pageB.on('pageerror', error => browserErrorsB.push(`pageerror: ${error.message}`));
  pageB.on('console', message => {
    if (message.type() === 'error') browserErrorsB.push(`console: ${message.text()}`);
  });
  await pageB.goto(origin, { waitUntil: 'networkidle', timeout: 30_000 });
  const cameraB = pageB.getByRole('article', { name: '摄像头直播' });
  await cameraB.waitFor({ state: 'visible', timeout: 30_000 });
  await cameraB.getByRole('button', { name: '播放直播' }).click();
  await cameraB.getByText('直播中', { exact: true }).waitFor({
    state: 'visible',
    timeout: 45_000,
  });

  for (const page of [pageA, pageB]) {
    await page.waitForFunction(() => {
      const video = document.querySelector('video[aria-label="摄像头实时画面"]');
      return video instanceof HTMLVideoElement && video.videoWidth === 1280
        && video.videoHeight === 720;
    }, null, { timeout: 45_000 });
  }

  const concurrent = await cameraStatus(pageA);
  assert.equal(concurrent.camera.pipeline, 'streaming');
  assert.equal(concurrent.camera.active_sessions, 2);

  // 关闭第一个页面：第二个页面继续播放。
  await cameraA.getByRole('button', { name: '停止直播' }).click();
  await cameraA.getByText('未播放', { exact: true }).waitFor({
    state: 'visible',
    timeout: 30_000,
  });
  await pageA.waitForFunction(async () => {
    const response = await fetch('/api/v1/camera/status', { cache: 'no-store' });
    if (!response.ok) return false;
    const body = await response.json();
    return body.camera.active_sessions === 1;
  }, null, { timeout: 30_000 });
  const remaining = await cameraStatus(pageB);
  assert.equal(remaining.camera.active_sessions, 1);
  await cameraB.getByText('直播中', { exact: true }).waitFor({ state: 'visible' });

  // 关闭第二个页面：全部清理。
  await cameraB.getByRole('button', { name: '停止直播' }).click();
  await cameraB.getByText('未播放', { exact: true }).waitFor({
    state: 'visible',
    timeout: 30_000,
  });
  await pageB.waitForFunction(async () => {
    const response = await fetch('/api/v1/camera/status', { cache: 'no-store' });
    if (!response.ok) return false;
    const body = await response.json();
    return body.camera.pipeline === 'stopped' && body.camera.active_sessions === 0;
  }, null, { timeout: 30_000 });
  await pageB.close();

  assert.deepEqual(browserErrorsB, []);
  return { concurrent_viewers: 2 };
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
      const concurrent = index === 0
        ? await verifyConcurrentViewers(context, origin, originName)
        : undefined;
      assert.deepEqual(browserErrors, []);
      results.push({ origin, desktop, mobile, concurrent });
    } finally {
      await context.close();
    }
  }
} finally {
  await browser.close();
}

console.log(JSON.stringify({ ok: true, results }, null, 2));
