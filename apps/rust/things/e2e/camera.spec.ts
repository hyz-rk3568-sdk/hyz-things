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

test("plays the camera anonymously and controls it as an administrator", async ({
  page,
  request,
}) => {
  await installCameraWebRtcMock(page);
  await page.goto("/");
  // 总览不自动启动摄像头；直播能力在摄像头一级页面内。
  await expect(
    page.getByText("视频不会自动启动", { exact: false }),
  ).toHaveCount(0);

  await page.getByRole("button", { name: "摄像头", exact: true }).click();
  const camera = page.getByRole("article", { name: "摄像头直播" });
  await expect(camera).toBeVisible();
  await expect(camera.getByText("可用", { exact: true })).toBeVisible();
  await expect(camera.getByText("已停止", { exact: true })).toBeVisible();
  await expect(
    camera.getByText(/3840 × 2160 · 30 fps · 20\.0 Mbps · h264/),
  ).toBeVisible();
  await expect(
    camera.getByText("LAN · 0 个会话", { exact: true }),
  ).toBeVisible();

  // 匿名观看：画面设置禁用，没有旋转按钮。
  const presetSelect = camera.getByRole("combobox", {
    name: "画面分辨率与码率",
  });
  await expect(presetSelect).toBeDisabled();

  const harnessSessionId = (createCount: number) =>
    `e2e-camera.${"a".repeat(46)}${createCount.toString(16).padStart(2, "0")}`;

  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        createCount: state.camera.create_count,
        offer: state.camera.last_offer_sdp,
        pipeline: state.camera.status.pipeline,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      createCount: 1,
      offer: "v=0\r\no=router-e2e-camera 1 1 IN IP4 127.0.0.1\r\n",
      pipeline: "streaming",
      activeSessions: 1,
      sessions: [harnessSessionId(1)],
    });

  await expect
    .poll(async () =>
      page.evaluate(() => ({
        playbackState: (window as any).__hyzCameraMediaSession.playbackState,
        title: (window as any).__hyzCameraMediaSession.metadata?.title ?? null,
      })),
    )
    .toEqual({ playbackState: "playing", title: "摄像头直播" });

  // 视频尚未有可播放数据时，画中画按钮保留但置灰；首帧就绪后才允许点击。
  await page.evaluate(() => (window as any).__hyzCameraSetVideoReady(false));
  const pipButton = camera.getByRole("button", { name: "画面准备中" });
  await expect(pipButton).toBeVisible();
  await expect(pipButton).toBeDisabled();
  await page.evaluate(() => (window as any).__hyzCameraSetVideoReady(true));
  await expect(
    camera.getByRole("button", { name: "进入画中画" }),
  ).toBeEnabled();
  await camera.getByRole("button", { name: "进入画中画" }).click();
  await expect(
    camera.getByRole("button", { name: "退出画中画" }),
  ).toBeVisible();
  await camera.getByRole("button", { name: "退出画中画" }).click();
  await expect(
    camera.getByRole("button", { name: "进入画中画" }),
  ).toBeVisible();

  const readMediaSession = () =>
    page.evaluate(() => {
      const mediaSession = (window as any).__hyzCameraMediaSession;
      return {
        playbackState: mediaSession.playbackState,
        title: mediaSession.metadata?.title ?? null,
        artist: mediaSession.metadata?.artist ?? null,
        handlers: Object.fromEntries(
          ["play", "pause", "stop"].map((action) => [
            action,
            typeof mediaSession.handlers[action] === "function",
          ]),
        ),
      };
    });

  await expect.poll(readMediaSession).toEqual({
    playbackState: "playing",
    title: "摄像头直播",
    artist: "hyz things",
    handlers: { play: true, pause: true, stop: true },
  });
  await expect(camera.getByRole("button", { name: "暂停直播" })).toBeVisible();
  await camera.getByRole("button", { name: "暂停直播" }).click();
  await expect(camera.getByRole("button", { name: "继续直播" })).toBeVisible();
  await expect(camera.getByText("直播已暂停", { exact: true })).toBeVisible();
  await expect.poll(readMediaSession).toMatchObject({
    playbackState: "paused",
  });
  await camera.getByRole("button", { name: "继续直播" }).click();
  await expect(camera.getByRole("button", { name: "暂停直播" })).toBeVisible();
  await expect(camera.getByText("直播已继续", { exact: true })).toBeVisible();
  await expect.poll(readMediaSession).toMatchObject({
    playbackState: "playing",
  });

  // 锁屏媒体控制的标准播放/暂停操作复用页面直播控制。
  await page.evaluate(() =>
    (window as any).__hyzCameraMediaSession.handlers.pause?.(),
  );
  await expect(camera.getByText("直播已暂停", { exact: true })).toBeVisible();
  await page.evaluate(() =>
    (window as any).__hyzCameraMediaSession.handlers.play?.(),
  );
  await expect(camera.getByText("直播已继续", { exact: true })).toBeVisible();
  await expect.poll(readMediaSession).toMatchObject({
    playbackState: "playing",
  });
  expect(
    await camera.locator("video").evaluate((video) => {
      const stream = (video as HTMLVideoElement).srcObject;
      return (
        stream instanceof MediaStream && stream.getVideoTracks().length === 1
      );
    }),
  ).toBe(true);
  await expect(camera.getByRole("button", { name: "旋转画面" })).toHaveCount(0);

  // 登录后回到摄像头视图：画面设置可用，切换视图时匿名会话已停止。
  await goToAppPage(page, "网络");
  await page.getByRole("button", { name: "管理员登录" }).click();
  await page.getByLabel("密码").fill("admin");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await page.getByLabel("当前密码").fill("admin");
  await page.getByLabel("新密码", { exact: true }).fill("router-e2e-password");
  await page.getByLabel("确认新密码").fill("router-e2e-password");
  await page.getByRole("button", { name: "修改密码" }).click();
  await expect(page.getByRole("button", { name: "上游 Wi-Fi (STA)" })).toBeVisible();

  await goToAppPage(page, "摄像头");
  await expect(camera.getByText("未播放", { exact: true })).toBeVisible();
  await expect(presetSelect).toBeEnabled();
  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();

  // 旋转是服务端媒体管线属性（videoflip）：断言桩端 rotation 与页面指标，而非 CSS class。
  const verifyRotation = async () => {
    const rotateButton = camera.getByRole("button", { name: "旋转画面" });
    await expect(rotateButton).toBeVisible();
    for (const degrees of [270, 180, 90, 0]) {
      await rotateButton.click();
      await expect
        .poll(
          async () =>
            (await readHarnessState(request)).camera.status.profile.rotation,
        )
        .toBe(`deg_${degrees}`);
      await expect(
        camera.getByText(`旋转 ${degrees}°`, { exact: false }),
      ).toBeVisible();
    }
  };
  await verifyRotation();

  // Switching the preset while playing stops the session and reopens with the new profile.
  const beforeSwitch = await readHarnessState(request);
  await presetSelect.selectOption("fhd1080p5m");
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        createCount: state.camera.create_count,
        closeCount: state.camera.close_count,
        width: state.camera.status.profile.width,
        height: state.camera.status.profile.height,
        bitrate: state.camera.status.profile.bitrate_bps,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      createCount: beforeSwitch.camera.create_count + 1,
      closeCount: beforeSwitch.camera.close_count + 1,
      width: 1920,
      height: 1080,
      bitrate: 5_000_000,
      activeSessions: 1,
      sessions: [harnessSessionId(beforeSwitch.camera.create_count + 1)],
    });
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();
  await expect(
    camera.getByText(/1920 × 1080 · 30 fps · 5\.0 Mbps · h264/),
  ).toBeVisible();

  await camera.getByRole("button", { name: "停止直播" }).click();
  await expect(camera.getByText("未播放", { exact: true })).toBeVisible();
  await expect(
    camera.getByRole("status").filter({ hasText: "摄像头直播已停止" }),
  ).toBeVisible();
  await expect.poll(readMediaSession).toEqual({
    playbackState: "none",
    title: null,
    artist: null,
    handlers: { play: false, pause: false, stop: false },
  });
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        closeCount: state.camera.close_count,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      closeCount: beforeSwitch.camera.close_count + 2,
      activeSessions: 0,
      sessions: [],
    });

  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).__hyzCameraRtcFail());
  await expect(
    camera.getByRole("status").filter({ hasText: "摄像头 WebRTC 连接已中断" }),
  ).toBeVisible();
  await expect
    .poll(async () => (await readHarnessState(request)).camera.sessions)
    .toEqual([]);

  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 360, height: 800 });
  await expectNoHorizontalOverflow(page);
  await verifyRotation();

  // 离开摄像头视图（去网络设置注销）会立即停止会话。
  await goToAppPage(page, "网络");
  await expect
    .poll(async () => (await readHarnessState(request)).camera.sessions)
    .toEqual([]);
  await page.getByRole("button", { name: "退出登录" }).click();
  await expect(page.getByRole("button", { name: "管理员登录" })).toBeVisible();
  await expect
    .poll(async () => (await readHarnessState(request)).camera.sessions)
    .toEqual([]);
});

test("enables full-duplex intercom with a real microphone track and cleans up", async ({
  page,
  request,
}) => {
  await installCameraWebRtcMock(page);
  await page.goto("/");
  await page.getByRole("button", { name: "摄像头", exact: true }).click();
  const camera = page.getByRole("article", { name: "摄像头直播" });
  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();

  // 播放时创建 video + audio 两个 transceiver（audio 为 sendrecv，对讲不重协商）。
  expect(
    await page.evaluate(() => (window as any).__hyzCameraTransceivers),
  ).toEqual(["video", "audio"]);
  // 对讲按钮只在播放且音频能力可用时出现，点击后切换麦克风状态。
  const talkButton = camera.getByRole("button", { name: "开启对讲" });
  await expect(talkButton).toBeVisible();

  // 点击开启对讲：真实 getUserMedia（fake device）提供麦克风轨道并挂载到 sender。
  await talkButton.click();
  await expect(camera.getByText("对讲已开启", { exact: false })).toBeVisible();
  await expect(camera.getByRole("button", { name: "关闭对讲" })).toBeVisible();
  await expect
    .poll(async () =>
      page.evaluate(() => (window as any).__hyzCameraReplaceTrackCalls),
    )
    .toEqual([{ kind: "audio", hasTrack: true }]);

  // 再次点击关闭对讲：摘下并停止麦克风轨道。
  await camera.getByRole("button", { name: "关闭对讲" }).click();
  await expect(camera.getByText("对讲已关闭", { exact: false })).toBeVisible();
  await expect(camera.getByRole("button", { name: "开启对讲" })).toBeVisible();
  await expect
    .poll(async () =>
      page.evaluate(() => (window as any).__hyzCameraReplaceTrackCalls),
    )
    .toEqual([
      { kind: "audio", hasTrack: true },
      { kind: "audio", hasTrack: false },
    ]);

  // 停止直播：会话关闭、管线与 session 归零。
  await camera.getByRole("button", { name: "停止直播" }).click();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        pipeline: state.camera.status.pipeline,
        activeSessions: state.camera.status.active_sessions,
      };
    })
    .toEqual({ pipeline: "stopped", activeSessions: 0 });
});

test("lets two anonymous viewers watch the camera concurrently", async ({
  context,
  request,
}) => {
  const first = await context.newPage();
  await installCameraWebRtcMock(first);
  await first.goto("/");
  await first.getByRole("button", { name: "摄像头", exact: true }).click();

  const cameraA = first.getByRole("article", { name: "摄像头直播" });
  await cameraA.getByRole("button", { name: "播放直播" }).click();
  await expect(cameraA.getByText("直播中", { exact: true })).toBeVisible();

  // 两个匿名观看者各自持有独立 viewer 令牌，可并发观看同一摄像头。
  const second = await context.newPage();
  await installCameraWebRtcMock(second);
  await second.goto("/");
  await second.getByRole("button", { name: "摄像头", exact: true }).click();
  const cameraB = second.getByRole("article", { name: "摄像头直播" });
  await cameraB.getByRole("button", { name: "播放直播" }).click();
  await expect(cameraB.getByText("直播中", { exact: true })).toBeVisible();

  const harnessSessionId = (createCount: number) =>
    `e2e-camera.${"a".repeat(46)}${createCount.toString(16).padStart(2, "0")}`;
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        createCount: state.camera.create_count,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      createCount: 2,
      activeSessions: 2,
      sessions: [harnessSessionId(1), harnessSessionId(2)],
    });
  await expect(first.getByText("直播中", { exact: true })).toBeVisible();
  await expect(second.getByText("直播中", { exact: true })).toBeVisible();

  // 关闭第一个页面：第二个页面的会话不受影响。
  await cameraA.getByRole("button", { name: "停止直播" }).click();
  await expect(cameraA.getByText("未播放", { exact: true })).toBeVisible();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({ activeSessions: 1, sessions: [harnessSessionId(2)] });
  await expect(cameraB.getByText("直播中", { exact: true })).toBeVisible();

  // 关闭第二个页面后全部清理。
  await cameraB.getByRole("button", { name: "停止直播" }).click();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({ activeSessions: 0, sessions: [] });
});
