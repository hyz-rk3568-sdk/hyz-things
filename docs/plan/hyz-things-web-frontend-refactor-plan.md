# hyz-things Web 前端结构与界面重构计划

## 状态

**完成。**

创建日期：2026-09-10。
完成日期：2026-09-11。

本文用于重构 `apps/rust/things` 的 Yew/WASM 管理门户。当前 Web 能力已经覆盖状态总览、网络配置、Mihomo 代理、Tailscale、Camera、Apps、系统状态和考试倒计时，但浏览器侧实现主要集中在 `src/web/main.rs`，API 调用、页面渲染、轮询、Camera/WebRTC、PiP、浏览器存储和倒计时等职责耦合较深。

本次计划先在不改变产品行为的前提下收敛 Web frontend 代码边界，再建立稳定的页面和 UI component 结构，最后调整管理门户的信息架构、响应式导航和视觉层级。

整个过程必须保持现有 HTTP、安全、router/camera process boundary 和用户可见能力。结构重构与 UI 行为变化分阶段进行，不执行一次性 SPA 重写。

## 1. 目标

本次计划完成以下事项：

- 将 `apps/rust/things/src/web/main.rs` 从集中式实现拆分为明确的 App、API、page、component 和 browser-side state/lifecycle 模块；
- 将 Camera/WebRTC、网络配置、Proxy、Tailscale、考试倒计时等高复杂度职责从 root component 中分离；
- 保持 Web 只通过现有受限 HTTP API 与 `hyz-things` native side 通信，不形成对 native inbound/outbound adapter 的依赖；
- 建立稳定、具有产品语义的共享 UI component，减少重复 Tailwind/daisyUI class 组合；
- 将桌面管理门户调整为稳定的 application shell 和一级导航，移动端提供等价的响应式导航；
- 重做 Overview，使 WAN、LAN/Wi-Fi、Proxy、Tailscale 和整体设备健康状态成为首页主要信息；
- 保持并整理 Playwright E2E，使测试围绕产品能力和可访问语义，而不是 Tailwind class 或 DOM 层级；
- 在结构重构阶段尽量保持现有 E2E 不变，只在真实 UI 行为变化阶段同步更新对应测试；
- 保持现有 Tailwind CSS 4 + daisyUI 5 技术栈，不引入新的前端框架。

完成本文计划不能改变 router、camera 或 wire protocol 的职责边界。

## 2. 非目标

本次不实现：

- React、Vue、Svelte 或其他 frontend framework 迁移；
- `hyz-router`、`hyz-camera` 的架构重构；
- `hyz-contract` wire protocol 重构；
- 为配合新 UI 任意新增或修改 router control operation；
- 修改网络 reconcile、Mihomo lifecycle、Tailscale lifecycle 或 Camera media pipeline；
- 在浏览器中直接访问 Unix socket、Linux interface、filesystem 或执行系统命令；
- 引入 Mihomo Controller、raw YAML、任意命令或其他新的管理面能力；
- 固件、Buildroot、kernel、rootfs 或 OTA 重构；
- 与本次页面结构和视觉层级无关的产品新功能。

如果实施过程中发现必须修改 HTTP contract、application use case 或 wire contract，应作为独立计划处理，不得夹带进本次 frontend refactor。

## 3. 固定架构边界

### 3.1 Web 边界

`apps/rust/things/src/web` 继续作为浏览器侧 driving adapter。

允许：

```text
Browser / Yew SPA
        │
        │ typed HTTPS/HTTP API
        ▼
hyz-things inbound HTTP
        │
        ▼
application ports/use cases
```

不得形成：

```text
Yew SPA
  ├── RouterControlClient
  ├── CameraUnixAdapter
  ├── Linux adapter
  ├── filesystem
  ├── shell command
  └── native application implementation
```

Web feature 必须继续保持与 native adapter 隔离。

### 3.2 HTTP 与安全边界

本次重构不得弱化：

- 管理员 session；
- bootstrap/password 语义；
- CSRF；
- same-origin 检查；
- fixed typed JSON mutation；
- body size limit；
- unknown-field 拒绝；
- Camera admin/viewer token 权限边界；
- LAN/Tailscale exact listener 语义；
- SPA/API fallback 和已有安全 header。

纯前端结构调整不得成为修改后端安全 contract 的理由。

### 3.3 用户行为

第一阶段结构拆分必须保持：

- API endpoint；
- request/response payload；
- mutation 顺序；
- loading/busy 行为；
- error 行为；
- network prepare/apply/confirm/cancel 语义；
- Proxy capability 语义；
- Tailscale mode/login/logout 语义；
- Camera session 生命周期；
- 浏览器 storage 中已有用户配置的兼容性。

## 4. 当前 Web frontend 问题

当前 `src/web/main.rs` 同时承担大量彼此独立的职责，包括：

- SPA bootstrap/root state；
- HTTP endpoint 与请求逻辑；
- authentication；
- PortalStatus 和轮询；
- network configuration；
- Proxy configuration；
- Tailscale；
- Camera viewer/session；
- WebRTC；
- PiP；
- browser storage；
- exam countdown；
- 页面和大量 UI rendering。

`src/web/ui.rs` 已经集中维护较多 Tailwind/daisyUI class token，这对视觉一致性有帮助，但这些 class constant 不能代替具有产品语义的 Yew component。

因此本次重构不以“单纯减少文件长度”为目标，而以职责边界、依赖方向和测试稳定性为判断标准。

## 5. 目标代码结构

目标结构允许根据实际依赖调整，但整体方向为：

```text
apps/rust/things/src/web/
├── main.rs
├── app.rs
├── api/
│   ├── mod.rs
│   ├── auth.rs
│   ├── status.rs
│   ├── network.rs
│   ├── proxy.rs
│   ├── tailscale.rs
│   ├── camera.rs
│   └── apps.rs
├── pages/
│   ├── mod.rs
│   ├── overview.rs
│   ├── network.rs
│   ├── proxy.rs
│   ├── tailscale.rs
│   ├── camera.rs
│   ├── apps.rs
│   └── system.rs
├── components/
│   ├── mod.rs
│   ├── app_shell.rs
│   ├── page_header.rs
│   ├── section_card.rs
│   ├── metric_card.rs
│   ├── status_badge.rs
│   ├── countdown_card.rs
│   ├── countdown_editor.rs
│   ├── loading.rs
│   └── topology.rs
├── hooks/
│   ├── mod.rs
│   ├── polling.rs
│   ├── camera.rs
│   └── countdown.rs
└── ui.rs
```

不得一次性创建完整空目录树再迁移。每个 module 只有在对应职责实际被抽离时创建。

考试倒计时只在代码层独立，不作为一级导航或独立管理页面。其 UI 由 Overview 中的 `CountdownCard` 和必要的 editor/modal/drawer 承载，倒计时状态不进入 portal-level global mega-state。

最终 `src/web/main.rs` 应主要承担 Web composition/bootstrap，例如：

```rust
mod api;
mod app;
mod components;
mod hooks;
mod pages;
mod ui;

fn main() {
    yew::Renderer::<app::App>::new().render();
}
```

具体 module 名允许调整，但 root 文件不应继续承载 Camera、network、Proxy、Tailscale 或 countdown 的主要实现。

## 6. 代码改动计划

### 6.1 建立重构基线

修改代码前先确认当前 host-side 状态。

至少记录：

- `make check-static` 当前结果；
- `apps/rust/things` native test 当前结果；
- strict Clippy 当前结果；
- `make things-e2e` 当前结果；
- Playwright 中已有失败或 flaky case。

如果某项因为本地工具、SDK 或浏览器依赖缺失无法执行，必须明确记录，不能把“未执行”记为“通过”。

不得为了本 Web 重构主动执行固件或 SDK 构建。

### 6.2 审计 Playwright contract

在移动 UI implementation 之前检查 `apps/rust/things/e2e`。

优先保留或建立以下 selector：

```text
role + accessible name
label
稳定可见文本
必要的 data-testid
```

避免测试依赖：

```text
Tailwind class
daisyUI implementation class
nth-child
非必要 DOM hierarchy
```

不能为了让测试变绿删除用户行为断言。
结构重构阶段，如果页面外部行为没有变化，现有 E2E 原则上也不应发生大规模修改。

### 6.3 抽离 HTTP API client

首先将 endpoint 和浏览器 HTTP request 相关职责从 `main.rs` 移到 `web/api`。

按能力划分：

```text
auth
status
network
proxy
tailscale
camera
apps
```

API module 负责 endpoint、request、typed serialization/deserialization 和 API-level error。

API module 不负责页面 rendering、modal、toast、navigation、页面布局或产品 UI state machine。

本阶段不得改变 endpoint 或 HTTP contract。

### 6.4 抽离考试倒计时

将以下职责从 root App 中分离：

- countdown target；
- 日期计算；
- custom target storage；
- remaining time 计算；
- countdown timer lifecycle。

纯日期计算应尽量设计成可确定性测试的纯函数，不直接把真实 wall clock 固化在测试中。

至少覆盖：future target、已过 target、boundary、custom target 和无效 storage data。

考试倒计时属于 Overview 辅助信息，不作为一级导航或独立页面。其计算、存储和 timer lifecycle 在代码层独立，UI 由 Overview 中的 CountdownCard 与 editor 承载。

### 6.5 抽离 Camera/WebRTC

Camera 属于本次风险最高的 frontend migration，必须独立完成和验证。

职责拆分为：

```text
camera API
camera browser/session lifecycle
camera page rendering
```

应从通用 App 中移出 viewer token、session create/close、SDP signaling、RTCPeerConnection、MediaStream、video element lifecycle、reconnect/retry 和 PiP。

不得改变：

- anonymous viewer token；
- admin-only profile/rotation；
- session ownership；
- SDP contract；
- Camera UDS/native contract。

Camera 迁移前后的对应 E2E 必须表达同一外部行为。

### 6.6 抽离 Network

Network 页面独立管理：

- observed/current state；
- STA scan；
- editable configuration；
- pending configuration；
- prepare；
- apply；
- confirm；
- cancel/error。

必须保持现有事务流程：

```text
edit
  ↓
prepare/apply
  ↓
pending
  ├── confirm
  └── cancel/rollback
```

UI 重构不能将 network mutation 简化为无法表达 pending/rollback 的普通 form submit。

### 6.7 抽离 Proxy

Proxy 页面继续明确区分：

- LAN TUN；
- 本机系统代理；
- Mihomo shared core；
- subscription/provider；
- proxy group/node；
- delay；
- device policy。

尤其不能因为新的 UI Card 或 Toggle 把 LAN TUN 和本机系统代理重新合并为单一 `proxy enabled` 概念。

### 6.8 抽离 Tailscale

将 status、peers、mode、login 和 logout 移入独立 API/page state。

Tailscale 页面继续只表达 Tailscale 自身状态，不得重新把 Mihomo 本机系统代理或 LAN TUN 状态混入 Tailscale capability。

### 6.9 抽离其余页面

在高复杂度模块完成后，再迁移 Overview、Apps、System、authentication shell 和 portal-level status。

迁移完成后 root App 只组合页面、全局认证和真正需要跨页面共享的状态。

不要为了减少 props 人为创建一个包含所有状态的全局 mega-context。

## 7. 共享 UI component

结构拆分并保持 E2E 全绿后，再从 `ui.rs` 中的重复视觉模式建立 Yew component。

优先候选：

```text
AppShell
PageHeader
SectionCard
MetricCard
StatusBadge
CountdownCard
CountdownEditor
LoadingState
ErrorState
EmptyState
```

component 应具有明确产品或界面语义。

不要建立只有一层 `<div>` 的 `GenericBox`、`GenericWrapper` 等抽象。

`ui.rs` 可以继续保存真正属于 design token 的 class，但常见完整模式应逐步由 component 表达。

## 8. Application shell 与导航

只有纯结构重构完成、共享 component 稳定并且 E2E 全绿后，才开始改变导航。

### 8.1 Desktop

桌面门户采用稳定一级导航。

目标信息架构：

```text
hyz things
├── Overview
├── Network
├── Proxy
├── Tailscale
├── Camera
├── Apps
└── System
```

考试倒计时不进入一级导航。

推荐 desktop shell：

```text
┌───────────────┬────────────────────────────┐
│ hyz things    │ page header / global state │
│               ├────────────────────────────┤
│ Overview      │                            │
│ Network       │                            │
│ Proxy         │        page content        │
│ Tailscale     │                            │
│ Camera        │                            │
│ Apps          │                            │
│ System        │                            │
└───────────────┴────────────────────────────┘
```

一级导航不应继续依赖所有页面同时作为大 Dashboard section 展开。

### 8.2 Mobile

移动 viewport 不强制显示桌面 sidebar。

应选择适合当前一级页面数量的 compact navigation，例如 drawer 或 compact menu。

必须保持：

- 当前页面可识别；
- 一级页面可达；
- 主要 mutation 不被遮挡；
- Camera video/control 可用；
- Network pending/confirm 流程可完成；
- 不产生页面级非预期横向滚动。

## 9. Overview 重构

Overview 的首要任务是让用户快速判断设备整体状态。

进入页面后应优先回答：

```text
Internet/WAN 是否正常？
LAN/Wi-Fi 是否正常？
Proxy 是否正常？
Tailscale 是否正常？
设备是否处于 degraded/unknown 状态？
```

### 9.1 第一层：核心健康状态

优先展示：

- Internet/WAN；
- LAN/Wi-Fi；
- Proxy；
- Tailscale。

统一使用稳定的 MetricCard/StatusBadge 语义。状态不能只依赖颜色，必须有文字。

### 9.2 第二层：网络拓扑

保留/重构现有 topology，使其表达真实关系：

```text
Internet
   │
  WAN
   │
RK3568
 ├── LAN / Wi-Fi clients
 ├── Mihomo
 └── Tailscale
```
拓扑只用于帮助理解当前状态，不增加装饰性 animation 或虚假的实时关系。

unknown、not-confirmed 和 degraded 不得渲染成正常 connected。

### 9.3 第三层：辅助信息

Camera、Apps、考试倒计时和其他辅助状态放到核心网络健康之后。

考试倒计时在 Overview 中使用 `CountdownCard` 展示最近目标和剩余时间；多个目标、自定义日期等编辑行为通过 editor/modal/drawer 完成，不创建独立一级页面。

这些辅助信息可以提供 summary/entry point，但不能抢占 WAN、LAN、Proxy 和 Tailscale 的视觉优先级。

## 10. 视觉系统

继续使用 Tailwind CSS 4 和 daisyUI 5，不引入第二套 component framework。

视觉调整方向：

- neutral surface；
- 减少高饱和大面积背景；
- 减少重复重阴影；
- 减少无状态含义的 accent color；
- 增加 typography hierarchy；
- 增加 whitespace；
- 统一 card spacing；
- 统一 form/control；
- 状态色只承担状态语义。

语义颜色保持：

```text
success  -> healthy/ready
warning  -> degraded/pending
error    -> failed
info     -> active/information
neutral  -> inactive/unknown
```

unknown/not-confirmed 不得使用与 healthy 相同的视觉语义。

### 10.1 Theme

结构和页面视觉稳定后，门户应能支持 dark/light semantic theme。

component 优先使用 daisyUI semantic token：

```text
base-100
base-200
base-300
base-content
primary
success
warning
error
```

避免在 component 中大量硬编码只适合 Dracula 的颜色。

现有 Dracula 可以继续作为 dark theme 基础，但不能成为 component contract。

## 11. Playwright E2E 重构

E2E 按产品 capability 组织，而不是按 Rust component 文件组织。

目标可以收敛为：

```text
e2e/
├── auth.spec.ts
├── overview.spec.ts
├── network.spec.ts
├── proxy.spec.ts
├── tailscale.spec.ts
├── camera.spec.ts
├── apps.spec.ts
└── system.spec.ts
```

实际文件名可以根据已有测试渐进迁移，不要求一次性 rename。

测试重点：

- 用户可以认证并进入门户；
- 用户可以观察 Portal/WAN/LAN 状态；
- network prepare/apply/confirm/cancel；
- LAN TUN 与本机系统代理保持独立；
- Proxy node/group 等已有行为；
- Tailscale mode/login/logout；
- Camera session create/close 和相关 viewer 行为；
- Apps/System 状态；
- desktop/mobile 导航；
- Overview 中考试倒计时可显示和编辑，但不要求独立导航页面。

不得把某个 Tailwind class、第二个 div、`nth-child(...)` 或 component 内部 DOM 顺序当作主要 assertion。

如果 UI redesign 改变用户可见导航或文案，对应 E2E 可以同步更新；这种修改必须与纯代码搬迁区分。

## 12. TDD 与实施顺序

遵循仓库 Red-Green-Refactor 规则。

对于纯结构迁移：

1. 先确认保护该行为的现有测试；
2. 如果覆盖不足，先增加最小行为测试；
3. 确认测试可以保护迁移边界；
4. 只移动一个 coherent responsibility；
5. 重新运行最小范围测试；
6. 整理新的模块边界；
7. 再运行相关完整测试；
8. 才进入下一模块。

对于 UI 行为变化：

1. Red：先调整/新增一个表达新外部行为的 E2E；
2. Green：最小实现新 UI 行为；
3. Refactor：收敛 component/class/state；
4. 再进入下一个行为。
推荐总体顺序：

```text
baseline
  ↓
E2E selector audit
  ↓
API extraction
  ↓
countdown extraction
  ↓
Camera extraction
  ↓
Network extraction
  ↓
Proxy extraction
  ↓
Tailscale extraction
  ↓
remaining page extraction
  ↓
full E2E
  ↓
shared UI components
  ↓
full E2E
  ↓
AppShell/navigation
  ↓
Overview redesign
  ↓
theme/responsive cleanup
  ↓
full host/frontend/E2E verification
```

不得把上述步骤压缩为一次“大规模重写”。

## 13. Host 验证

按静态检查优先执行。

计划使用：

```sh
git diff --check

make check-static

cargo fmt \
  --manifest-path apps/rust/things/Cargo.toml \
  --all -- --check

cargo test \
  --locked \
  --manifest-path apps/rust/things/Cargo.toml \
  --features native

cargo clippy \
  --locked \
  --manifest-path apps/rust/things/Cargo.toml \
  --all-targets \
  --features e2e \
  -- -D warnings
```

Web bundle：

```sh
make things-frontend
```

浏览器行为：

```sh
make things-e2e
```

普通 host 验证不得：

- 操作真实接口；
- 修改真实 route/firewall；
- 依赖真实 WAN；
- 依赖真实 Mihomo provider；
- 使用真实家庭网络凭据；
- 修改 `/run` 或 `/userdata` 产品状态。

`hyz-things-e2e` loopback Axum harness 继续作为浏览器 E2E 的产品替身。

### Host 结果

| 项目 | 状态 | 证据/备注 |
| --- | --- | --- |
| `git diff --check` | 通过 | PR CI #95 / run `34506198144` 的 `Check committed diff` 通过。 |
| `make check-static` | 通过 | 同一 run 的 `Run source and configuration checks` 直接执行该 target 并通过。 |
| things format | 通过 | 同一 run 的 `cargo fmt --manifest-path apps/rust/things/Cargo.toml --all -- --check` 通过。 |
| things native tests | 通过 | 同一 run 的 `cargo test --locked --manifest-path apps/rust/things/Cargo.toml --features native` 通过。 |
| things strict Clippy | 通过 | 同一 run 的 `cargo clippy --locked --manifest-path apps/rust/things/Cargo.toml --all-targets --features e2e -- -D warnings` 通过。 |
| `make things-frontend` | 未直接执行；等价构建通过 | CI 使用该 target 的核心构建路径 `tools/build-frontend-bundle.sh` 生成 deterministic bundle，并成功上传/复用于 E2E；仓库本地 `make` target 额外要求 `.tools/trunk`，GitHub runner 不走该本地包装层。 |
| `make things-e2e` | 未直接执行；等价浏览器路径通过 | CI 在已验证 bundle 上直接执行该 target 的核心命令 `npm run test:e2e`，使用 loopback `hyz-things-e2e` harness；日志为 `Running 32 tests using 1 worker`、`32 passed (2.3m)`。 |

最终 UI/主题/视觉 checkpoint 为 human commit `5d56cf69437c06543de842dfed1f2ab8067a3310`，PR CI #95 / run `34506198144` 的 Static + unit + frontend 与 Playwright E2E 两个 job 均通过。浏览器侧纯逻辑、native tests、strict Clippy、deterministic frontend bundle 与 capability-oriented Playwright baseline 均保持绿色。

合并前清理一次性实施基础设施：删除临时 `web refactor driver` workflow、`web-refactor*.py` migration 脚本和过程 progress 文档；长期 `tools/check-e2e-suites.py` 保留为正式 CI contract。最终清理提交仍必须通过正常 PR CI 后才能合并。

## 14. 分阶段通过标准

### 14.1 结构阶段

通过标准：

- `main.rs` 不再承载主要 page implementation；
- API request 与页面 rendering 分离；
- Camera/WebRTC 已脱离通用 root component；
- Network、Proxy、Tailscale 各有明确 frontend boundary；
- countdown 已与 portal 核心状态分离，但仍由 Overview 承载 UI；
- 无新的 native adapter dependency；
- 用户行为没有非计划变化；
- frontend build 和现有 E2E 通过。

### 14.2 UI component 阶段
通过标准：

- 高频重复状态 UI 使用统一 component；
- status semantics 一致；
- loading/error/unknown 有统一表达；
- component 不暴露不必要的 Tailwind implementation detail 给测试；
- accessibility semantics 不低于原实现。

### 14.3 导航和 Overview 阶段

通过标准：

- desktop 一级导航清晰；
- mobile 可以访问全部主要页面；
- Overview 首屏优先表达网络和设备健康；
- countdown 作为辅助信息存在，但不占用一级导航；
- unknown/degraded/not-confirmed 不会被误显示为 healthy；
- 主要现有操作仍可从新结构完成；
- desktop/mobile E2E 通过。

## 15. 回归与失败处理

如果纯结构重构导致 E2E 失败，必须先判断是产品行为发生变化，还是测试依赖了 implementation detail。

如果产品行为无意变化，应修复产品代码。

如果测试只依赖 Tailwind class、DOM hierarchy 等不稳定实现，应在保持同一用户行为 assertion 的情况下修复 selector。

不得：

- 删除失败测试；
- 降低安全 assertion；
- 将真实错误改成无条件 success；
- 用固定 sleep 掩盖异步生命周期问题；
- 为测试添加产品环境不存在的特殊 bypass。

如果某个抽离步骤同时要求大量无关页面变化，应停止该步骤并缩小迁移边界。

## 16. 提交边界

建议保持小而可验证的提交，例如：

```text
test(web): stabilize browser selectors
refactor(web): extract browser API modules
refactor(web): extract countdown state
refactor(web): extract camera frontend lifecycle
refactor(web): extract network page
refactor(web): extract proxy page
refactor(web): extract tailscale page
refactor(web): extract remaining portal pages
refactor(web): introduce shared status components
refactor(web): introduce application shell
refactor(e2e): organize portal tests by capability
feat(web): redesign overview
feat(web): support semantic light and dark themes
```

具体提交数量不作为目标。

关键要求是：

- 纯 refactor 不夹带视觉 redesign；
- selector cleanup 不夹带产品行为变化；
- 一个失败可以定位到有限改动范围；
- 每个阶段可以独立回滚。

## 17. 完成标准

只有满足以下条件才能把本文状态改为完成：

- Web frontend 职责已从集中式 `main.rs` 收敛到明确模块；
- root App 只拥有真正需要跨页面共享的状态和 composition；
- Camera、Network、Proxy、Tailscale 不再作为大型实现混杂在 root 文件；
- countdown 已完成代码层职责拆分，但继续作为 Overview 辅助组件而非独立一级页面；
- reusable UI component 已覆盖主要重复模式；
- desktop/mobile navigation 已按新信息架构完成；
- Overview 优先反映设备和网络真实健康状态；
- Tailwind/daisyUI 继续作为唯一主要视觉技术栈；
- Playwright 不以 Tailwind class 和脆弱 DOM hierarchy 作为主要 contract；
- 现有 HTTP、安全和 process boundary 没有弱化；
- 所有实际执行的 static、unit、frontend 和 E2E 验证结果已记录；
- 未执行的验证项目明确标记，而不是推断通过。

## 18. 不在范围内

完成本文后仍不代表以下能力完成：

- 新的 router/network feature；
- Ethernet/double-uplink；
- PPPoE/VLAN；
- DNS center；
- Mihomo Controller；
- Tailscale 新 transport 能力；
- Camera audio/intercom；
- firmware/OTA 重构；
- RK3568 板端完整网络验收。

这些能力继续由各自的 user story、architecture 或独立 plan 文档跟踪。
