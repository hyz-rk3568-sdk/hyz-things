# hyz-things

`hyz-things` 是 RK3568 产品上的**门户与管理进程**，按用户需求以“hyz things”个人网站形式承载设备管理面。它不参与路由/防火墙/NAT，而是通过 root-only Unix socket 驱动独立无头核心 `hyz-router`，并通过 `/run/hyz-camera/control.sock` 驱动独立媒体进程 `hyz-camera`。

- 板端程序：`/usr/bin/hyz-things`（init：`/etc/init.d/S83hyz-things`）
- 管理地址：`http://192.168.8.1:8080`（LAN 固定监听；Tailscale 认证后精确绑定单个 Tailscale IPv4）
- 启动前置：等待 `/run/hyz-router/ready` 标记，之后才绑定 HTTP
- 共享 wire 契约：`apps/rust/contract`（`hyz-contract`）
- 详细设计、架构图和板端验证记录：[docs/architecture.md](../../../docs/architecture.md)

## 能力

- 代理设置页保留 LAN TUN 开关，并将原代理开关迁移为“本机系统代理”：使用 Mihomo `127.0.0.1:7890` 提供本机 HTTP/HTTPS 显式代理，不接管所有本机流量，也不控制 Tailscale。
- 管理员认证：Argon2id 凭据持久化在 `/userdata/hyz-router/admin/credential.json`，bootstrap 密码强制改密、会话与 CSRF 边界沿用原安全模型。
- 状态聚合 `PortalStatus`：router 不可用时返回明确 degraded 快照，health 保持纯存活探针。
- 固定 LAN 监听与 Tailscale exact listener 管理：地址变化时先停旧 listener 再绑定新地址，永不回退 `0.0.0.0`。
- 非 router 应用热推送：`tools/deploy-app.sh` 只支持对 `hyz-things` 和 `hyz-camera` 推送新 ELF，不重启其他应用，并在停止服务前做协议版本兼容性检查。router 的开发热替换使用独立的受控流程：USB ADB 执行 `stop → 原子替换 → start`，网络 ADB 执行 `原子替换 → reboot`；正式发布仍通过 OTA。

## 代码结构

```text
src/domain/                 纯状态、值对象与不变量（大量 re-export 自 hyz-contract）
src/application/            门户用例：admin、camera client、status、ports
src/adapters/inbound/       Axum HTTP
src/adapters/outbound/      RouterControlClient、CameraUnixAdapter、AdminFileAdapter、Tailscale listener、storage
src/web/                    Yew/WASM 前端（web feature）
src/e2e/                    host-only loopback harness（e2e feature，bin hyz-things-e2e）
frontend/                   Trunk HTML/CSS 入口（Tailwind CSS 4 + daisyUI 5）
tools/                      deterministic bundle、e2e server、deploy-app.sh 与测试
tests/                      HTTP 安全边界集成测试
```

依赖方向保持为 `adapters -> application -> domain`。浏览器、`hyz-things` 自身都不能直接执行 Linux 命令；所有平台能力经由 router 的 root-only control socket 或 camera 的 control socket。

## 构建与检查

最低 Rust 版本为 `1.85`，前端工具要求 Node.js `20` 或更新版本。native 实现只支持 Linux。

```sh
cargo test --locked --features native
cargo clippy --locked --all-targets --features e2e -- -D warnings
```

前端 deterministic bundle 与 Playwright 浏览器测试从仓库根目录执行：

```sh
make things-frontend
make things-e2e
make things-app
```

`things-frontend` 先运行 `npm run build:css`（Tailwind/daisyUI 静态 CSS）再运行 Trunk release WASM 构建，产物为确定性的 `hyz-things-frontend.tar`；`things-e2e` 使用 host-only loopback Axum harness 运行 Playwright，不执行产品网络命令；`things-app` 将 bundle 嵌入 ELF 并完成 AArch64 native 构建。

## 运行

`hyz-things daemon` 只能在具备完整产品运行时的目标板上以 root 启动，且必须由 `S83hyz-things` 在 router 就绪后拉起。普通开发机上可用 `hyz-things-e2e`（非默认 `e2e` feature）预览页面，它只绑定 loopback 并使用 fake ports。
