import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import {
  expectNoHorizontalOverflow,
  goToAppPage,
  harnessOrigin,
  installCameraWebRtcMock,
  loginAsAdmin,
  readHarnessState,
  resetHarness,
  webOrigin,
} from "./fixtures";
import {
  readCountdownTotal,
  readCustomCountdownTotal,
  realTouchSwipe,
  swipePortal,
} from "./support";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test("serves every KaTeX font referenced by the generated stylesheet", async ({ request }) => {
  const root = await request.get("/");
  const html = await root.text();
  const cssPath = html.match(/href="(\/katex\.min-[^"]+\.css)"/)?.[1];
  expect(cssPath).toBeTruthy();

  const stylesheet = await request.get(cssPath!);
  expect(stylesheet.ok()).toBeTruthy();
  const fontPaths = [
    ...new Set(
      [...(await stylesheet.text()).matchAll(/url\((?:["']?)(fonts\/KaTeX_[^)"']+)/g)].map(
        (match) => `/${match[1]}`,
      ),
    ),
  ];
  expect(fontPaths.length).toBeGreaterThan(0);

  for (const fontPath of fontPaths) {
    const font = await request.get(fontPath);
    expect(font.ok(), fontPath).toBeTruthy();
    expect(await font.body(), fontPath).not.toHaveLength(0);
  }
});

test("serves the generated bundle through the strict production-shaped HTTP boundary", async ({
  request,
}) => {
  const root = await request.get("/");
  expect(root.ok()).toBeTruthy();
  const csp = root.headers()["content-security-policy"] ?? "";
  expect(csp).toContain("script-src 'self' 'wasm-unsafe-eval'");
  expect(csp).not.toContain("'unsafe-inline'");
  expect(csp).not.toContain("script-src 'self' 'unsafe-eval'");

  const html = await root.text();
  expect(html).not.toMatch(/<style[\s>]/i);
  expect(html).not.toMatch(/<script(?![^>]*\bsrc=)[^>]*>\s*\S/i);
  const bootstrapPath = html.match(
    /src="(\/router-bootstrap\.js\?v=[0-9a-f]{16})"/,
  )?.[1];
  expect(bootstrapPath).toBeTruthy();

  const bootstrap = await request.get(bootstrapPath!);
  expect(bootstrap.ok()).toBeTruthy();
  expect(bootstrap.headers()["content-type"]).toContain("text/javascript");

  const staleAsset = await request.get("/router-web-stale.css");
  expect(staleAsset.status()).toBe(404);
  expect(await staleAsset.text()).not.toContain("<!doctype html>");

  const unknownApi = await request.get("/api/v1/not-a-route");
  expect(unknownApi.status()).toBe(404);
  expect(unknownApi.headers()["content-type"]).toContain("application/json");

  const panel = await request.get("/api/v1/panel");
  const csrf = ((await panel.json()) as { csrf_token: string }).csrf_token;
  const anonymousLanTun = await request.post("/api/v1/control/proxy/lan-tun", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { enabled: true },
  });
  expect(anonymousLanTun.status()).toBe(401);
  const anonymousTailscaleProxy = await request.post(
    "/api/v1/control/proxy/local-system",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
      data: { enabled: true },
    },
  );
  expect(anonymousTailscaleProxy.status()).toBe(401);
  const anonymousTailscalePeers = await request.get("/api/v1/tailscale/peers");
  expect(anonymousTailscalePeers.status()).toBe(401);
  const anonymousCameraStatus = await request.get("/api/v1/camera/status");
  expect(anonymousCameraStatus.status()).toBe(401);

  const anonymousCameraVolume = await request.post("/api/v1/control/camera/volume", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { volume_percent: 42 },
  });
  expect(anonymousCameraVolume.status()).toBe(401);

  const invalidCameraVolume = await request.post("/api/v1/control/camera/volume", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { volume_percent: 101 },
  });
  expect(invalidCameraVolume.status()).toBe(401);

  const unknownCameraVolumeField = await request.post("/api/v1/control/camera/volume", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { volume_percent: 42, extra: true },
  });
  expect(unknownCameraVolumeField.status()).toBe(401);

  const missingCameraVolumeCsrf = await request.post("/api/v1/control/camera/volume", {
    headers: { Origin: webOrigin },
    data: { volume_percent: 42 },
  });
  expect(missingCameraVolumeCsrf.status()).toBe(403);

  const foreignCameraVolumeOrigin = await request.post("/api/v1/control/camera/volume", {
    headers: { Origin: "http://evil.example", "X-HYZ-CSRF": csrf },
    data: { volume_percent: 42 },
  });
  expect(foreignCameraVolumeOrigin.status()).toBe(403);

  const cameraVolumeGet = await request.get("/api/v1/control/camera/volume");
  expect(cameraVolumeGet.status()).toBe(405);

  const anonymousCameraCreate = await request.post(
    "/api/v1/control/camera/session/create",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
      data: { offer_sdp: "v=0\r\n" },
    },
  );
  expect(anonymousCameraCreate.status()).toBe(401);
  const apps = await request.get("/api/v1/apps");
  expect(apps.status()).toBe(200);
  const appsBody = (await apps.json()) as {
    apps: Array<{
      name: string;
      sha256: string | null;
      deployed_at_unix_ms: number | null;
    }>;
  };
  expect(appsBody.apps.map((app) => app.name)).toEqual([
    "camera",
    "router",
    "things",
  ]);
  expect(appsBody.apps.every((app) => app.sha256 !== null)).toBeTruthy();
  expect(
    appsBody.apps.every(
      (app) =>
        typeof app.deployed_at_unix_ms === "number" &&
        app.deployed_at_unix_ms > 0,
    ),
  ).toBeTruthy();
  // Anonymous token issuance is removed; legacy 64-character tokens
  // must never authorize new camera sessions.
  const viewerToken = await request.post("/api/v1/camera/viewer-token", {
    headers: { Origin: webOrigin, "Content-Type": "application/json" },
    data: {},
  });
  expect([404, 405]).toContain(viewerToken.status());
  const legacyToken = "a".repeat(64);
  const legacyViewerCreate = await request.post(
    "/api/v1/control/camera/session/create",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": legacyToken },
      data: { offer_sdp: "v=0\\r\\n" },
    },
  );
  expect(legacyViewerCreate.status()).not.toBe(200);
  const legacyViewerClose = await request.post(
    "/api/v1/control/camera/session/close",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": legacyToken },
      data: { session_id: "invalid" },
    },
  );
  expect(legacyViewerClose.status()).not.toBe(200);
  const legacyViewerProfile = await request.post(
    "/api/v1/control/camera/profile",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": legacyToken },
      data: { preset: "fhd1080p5m" },
    },
  );
  expect(legacyViewerProfile.status()).toBe(403);
  const removedProxyMode = await request.post("/api/v1/control/proxy/mode", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { mode: "tun" },
  });
  expect(removedProxyMode.status()).toBe(405);
  const anonymousProxySelection = await request.post(
    "/api/v1/control/proxy/selection",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
      data: { group: "自动选择", proxy: "新加坡" },
    },
  );
  expect(anonymousProxySelection.status()).toBe(401);

  const foreignOrigin = await request.post("/api/v1/control/display", {
    headers: {
      Origin: "http://evil.example",
      "X-HYZ-CSRF": csrf,
    },
    data: { enabled: false },
  });
  expect(foreignOrigin.status()).toBe(403);

  const missingCsrf = await request.post("/api/v1/control/display", {
    headers: { Origin: webOrigin },
    data: { enabled: false },
  });
  expect(missingCsrf.status()).toBe(403);

  const displayBefore = (await readHarnessState(request)).panel.display.data;
  const oversized = await request.post("/api/v1/control/display", {
    headers: {
      Origin: webOrigin,
      "X-HYZ-CSRF": csrf,
      "Content-Type": "application/json",
    },
    data: JSON.stringify({
      enabled: true,
      brightness: 1,
      padding: "x".repeat(5_000),
    }),
  });
  expect(oversized.status()).toBe(413);
  expect((await readHarnessState(request)).panel.display.data).toEqual(
    displayBefore,
  );

  const login = await request.post("/api/v1/auth/login", {
    headers: {
      Origin: webOrigin,
      "X-HYZ-CSRF": csrf,
      "Content-Type": "application/json",
    },
    data: { password: "admin" },
  });
  expect(login.status()).toBe(200);
  const administratorCookie = login.headers()["set-cookie"] ?? "";
  expect(administratorCookie).toContain("HttpOnly");
  expect(administratorCookie).toContain("SameSite=Strict");
  expect(administratorCookie).toContain("Path=/");
  expect(administratorCookie).toContain("Max-Age=2592000");

  // Initial bootstrap login is restricted until the password is changed.
  expect((await request.get("/api/v1/camera/status")).status()).toBe(403);
  const passwordChange = await request.post("/api/v1/auth/password", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: {
      current_password: "admin",
      new_password: "router-e2e-password",
    },
  });
  expect(passwordChange.status()).toBe(200);
  expect((await request.get("/api/v1/camera/status")).status()).toBe(200);

  const authenticatedVolume = await request.post("/api/v1/control/camera/volume", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { volume_percent: 42 },
  });
  expect(authenticatedVolume.status()).toBe(200);
  expect(await authenticatedVolume.json()).toEqual({ applied: true });
  await expect
    .poll(async () => (await readHarnessState(request)).camera.status.audio.volume_percent)
    .toBe(42);
  const invalidAuthenticatedVolume = await request.post("/api/v1/control/camera/volume", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { volume_percent: 101 },
  });
  expect(invalidAuthenticatedVolume.status()).toBe(400);
  expect((await invalidAuthenticatedVolume.json()).error.code).toBe("camera_invalid_request");

  const administratorCreate = await request.post(
    "/api/v1/control/camera/session/create",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
      data: { offer_sdp: "v=0\\r\\n" },
    },
  );
  expect(administratorCreate.status()).toBe(200);
  const { session_id } = await administratorCreate.json();
  const administratorClose = await request.post(
    "/api/v1/control/camera/session/close",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
      data: { session_id },
    },
  );
  expect(administratorClose.status()).toBe(200);

  const logout = await request.post("/api/v1/auth/logout", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: {},
  });
  expect(logout.status()).toBe(200);
  expect((await request.get("/api/v1/camera/status")).status()).toBe(401);
});
