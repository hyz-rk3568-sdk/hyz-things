# hyz-things Web 页面数据、刷新与设备操作优化计划

## 状态

**代码已实施，目标板验收待完成。**

创建日期：2026-09-14。
实施完成日期：2026-09-15。

本文承接 [Web 前端结构与界面重构计划](hyz-things-web-frontend-refactor-plan.md)，定义门户页面数据、刷新调度、设备操作与 URL 导航的实施范围。功能代码在独立分支 `feat/web-page-data-and-devices`、PR #9 中完成。

本状态只代表代码与 GitHub Actions/loopback harness 自动化已经完成，不代表真实 RK3568 目标板已经验收。目标板验收仍由用户执行，不能用 CI 替代。

## 1. 本轮目标

本轮最终实现以下四项：

1. 设置按真实资源、页面与模块独立加载、独立报错和重试。
2. 只按当前页面请求必要数据；浏览器后台暂停或降频，恢复可见、focus、online 时立即刷新当前页；统一表达最后成功更新时间。
3. 将设备列表、详情、活动、策略修改串成一个完整设备工作流，并精简网络/代理/Tailscale 页面职责。
4. 将页面和设备详情位置同步到 typed hash route，支持刷新、浏览器前进/后退和登录后保留原目标。

本轮不做首页信息层级改版，不扩展代理规则、DNS、OTA、摄像头媒体能力或学习功能。现有独立 Study 页面保留。

## 2. 最终页面信息架构

| 页面 | 最终职责 | 路由 |
| --- | --- | --- |
| 首页 | 保持现有内容，接入页面级资源刷新 | `#/overview` |
| 学习 | 倒计时、备考与 PiP；不因隐藏门户 section 产生管理请求 | `#/study` |
| 网络 | 管理员认证、STA/AP、网络当前配置、pending/确认/回滚 | `#/network` |
| 代理 | LAN TUN、本机系统代理、节点、手动延迟测试、订阅 | `#/proxy` |
| 设备 | 设备列表、筛选、详情、Mihomo 活动、名称与策略 | `#/devices` |
| Tailscale | 状态、peers、模式、登录/注销 | `#/tailscale` |
| 摄像头 | 保持现有观看、对讲、画面控制与权限 | `#/camera` |
| 应用 | 保持应用状态，并支持模块级重试 | `#/apps` |
| 系统 | 保持现有系统信息与管理能力 | `#/system` |

旧 `#/activity` 仅作为兼容别名接受，随后使用 replace 规范化为 `#/devices`。

## 3. URL 导航与恢复

最终路由由 `src/web/navigation.rs` 统一解析和生成：

- 空 hash 或非法 hash 规范化到首页，不制造重定向循环。
- 设备详情使用 `#/devices/<device-id>`；当前实现使用规范化 MAC。
- 设备筛选可放在 hash query 中，例如 `#/devices?q=...`。
- 普通页面导航、滑动导航、设备打开/关闭、浏览器前进/后退使用同一 typed route 操作。
- 刷新页面恢复当前页面、设备详情和 URL 中筛选。
- 从列表打开详情后关闭，恢复列表筛选、滚动位置和打开按钮焦点。
- 直接打开设备详情深链接，关闭时回设备列表，不盲目 `history.back()` 离开站点。
- 未登录访问受保护深链接时在当前路由就地登录；登录成功继续原目标。
- 401 清理管理员缓存但保留目标路由。
- Study 页面不会因路由切换被无意重建；摄像头原有离页会话释放逻辑不因 typed route 被绕过。

## 4. 独立资源状态

`src/web/resources.rs` 以真实 HTTP 资源维护独立缓存，而不是继续维护一次性“全部设置加载成功”的大元组。

每个资源维护：

- 最近一次成功数据；
- loading/refreshing；
- 最近错误；
- 最后成功接收时间；
- 请求代次 `request_id`；
- 管理员认证作用域 `auth_epoch`；
- stale/待重新确认状态。

主要资源边界包括：

- `/api/v1/status`
- `/api/v1/panel`
- `/api/v1/network/config`
- `/api/v1/network/pending`
- `/api/v1/proxy/subscription`
- `/api/v1/proxy/device-policies`
- `/api/v1/tailscale`
- `/api/v1/tailscale/peers`
- `/api/v1/apps`

独立请求可以并行完成；某一资源失败不会阻断其他资源首次提交。已有成功数据刷新失败时继续显示旧数据，并明确标记失败和最后成功更新时间。

## 5. 页面订阅与刷新调度

最终页面依赖如下：

| 页面 | 自动读取资源 | 明确不自动读取/触发 |
| --- | --- | --- |
| 首页 | status、panel | 管理设置、设备、peers |
| 学习 | 本地 Study 状态 | 设备、订阅、peers、代理测速 |
| 网络 | panel；登录后 network config、pending | 订阅、设备、peers、代理测速 |
| 代理 | panel、status；登录后 subscription | network config、设备、peers；订阅 refresh/代理 delay mutation |
| 设备 | panel；登录后 device policy/activity snapshot | network config、订阅、peers |
| Tailscale | panel；登录后 status、peers | 订阅、设备 |
| 摄像头 | panel + 现有 camera 会话资源 | network config、订阅、设备 |
| 应用 | apps | 无关管理配置 |
| 系统 | status、panel | 无关管理配置 |

调度规则已经落地：

- 同一资源最多一个读取请求在途。
- 页面退出后不再由该页面持续发周期请求。
- 进入当前页立即获取需要资源。
- 可见动态页面沿用约 2 秒的动态观测基线。
- 浏览器 hidden 时普通页面刷新暂停或使用不少于 60 秒的调度，不继续 2 秒全局轮询。
- `visibilitychange` 恢复、focus、online 会触发当前页立即刷新。
- 失败采用有上限的退避；页面手动重试不受旧退避锁死。
- 配置类资源主要由进入页面、手动重试和 mutation 后回读驱动。
- 普通 polling 永不自动 POST 代理节点 delay refresh，也不自动 POST subscription refresh。

## 6. mutation、并发与认证语义

### 6.1 通用规则

- busy/error 属于相关操作或资源，不使用全局 settings busy 锁死无关页面。
- 写成功只失效/回读相关资源。
- 写成功而回读失败显示“已保存，状态刷新失败”，不能伪装成写失败。
- 旧读取请求不能覆盖新 mutation、新路由或新的登录状态。
- 管理员 logout/401 增加 `auth_epoch` 并清除受保护缓存；旧认证作用域返回后不能重新填回缓存。
- 403/CSRF 错误不进入无限重新登录循环。

### 6.2 网络

网络 current config 与 pending 独立读取；pending 未知时仍允许读取其他模块，但危险 AP 事务操作会被禁用。现有 prepare/apply/confirm/cancel/rollback 语义保留。

### 6.3 设备 generation 冲突

设备策略更新继续使用 `expected_generation`。409 时：

- 刷新服务器当前状态；
- 本地正在编辑的 label/policy 草稿不被后台状态覆盖；
- 用户核对后再保存，不自动重放旧整表。

### 6.4 Tailscale mutation 与旧 GET

Tailscale 写操作开始时暂停该页状态/peers 周期调度，并推进 Tailscale 资源请求代次。这样：

- mutation 前已在途的旧 GET 返回后因代次过旧不能覆盖写结果；
- mutation 在途期间不再启动新的状态 GET；
- 写完成后页面 effect 重新进入，立即读取当前 status/peers。

这一行为有专门 Playwright 请求计数验证。

## 7. 设备页与详情

设备页替代旧“活动”页，并把设备策略从网络页迁入详情。

### 7.1 设备集合

列表合并三类来源：

1. 当前 DHCP/LAN client observation；
2. 已配置但当前未发现的 policy entry；
3. 只在 activity 中出现的 MAC。

显示名称优先级：

1. 已配置 label；
2. DHCP/观测 hostname；
3. activity device name；
4. MAC。

在线状态只以当前 LAN client observation 的 `associated` 为权威。activity 记录不能把未关联设备推断为“在线”。配置存在但当前没有 client observation 时显示“当前未发现”。

### 7.2 设备详情

桌面使用侧边抽屉，窄屏使用全屏详情。内容包括：

- 名称；
- MAC / IP；
- 在线、离线或未知；
- 当前设备 policy；
- Mihomo 可观测上传/下载；
- 有界近期连接、目标、rule、chains/出口；
- label/policy 编辑；
- 清除名称/自定义策略；
- 手工 MAC 添加入口仍保留。

连接历史按 `connection_id` 去重；没有 activity 只说明 Mihomo 当前没有记录，不等于设备没有访问互联网。

### 7.3 “已保存”与“已生效”

这两个状态严格分开：

- device-policy mutation 响应成功只证明配置已保存；mutation 返回的 `effective` 本身不能作为最终运行态证明。
- 成功回读新的 `/api/v1/proxy/device-policies` snapshot 后，后端 `DevicePolicyApplication::effective_for(config)` 会使用**当前 config 的 `direct_macs`**和真实代理 observation 做 `ready_for(...)` 比对。
- 因此只有 fresh GET snapshot 的 `effective=true` 时，前端才显示“已生效”。
- 若写成功但 fresh GET 失败，显示“已保存，状态刷新失败”。
- fresh snapshot `effective=false` 时保留“已保存/等待 TUN”等保守表达。

## 8. 页面精简结果

- Network 不再渲染代理订阅或设备策略。
- Proxy 独占 subscription 设置和手动 subscription refresh。
- Devices 独占设备列表、详情、设备 policy 与 activity。
- Tailscale 独占 Tailscale status/peers/mutations。
- Study 保持独立页面。
- 实施期 `app_v2.rs`、`network_v2.rs` 和旧 `activity.rs` 已清理；正式源码入口恢复为 `app.rs`、`network.rs`、`devices.rs`。

## 9. 可访问性与交互约束

- 设备详情具有 `role=dialog`、可访问名称、Escape 关闭和焦点返回。
- 移动端详情全屏，桌面详情抽屉。
- 列表筛选保留在 URL；关闭详情恢复滚动位置。
- 失败/成功反馈继续使用现有 role/status/alert 语义。
- 页面切换和 hidden portal section 不制造重复 id 的活跃控件或额外 timer。

## 10. TDD 与自动化验证记录

本轮按仓库 `AGENTS.md` 要求先验证 Red，再完成 Green。

### 10.1 Red 证据

示例 Red commit：`bc78e8e36c30e6dee328b87cd9427e7cb70ec756`。
Portal CI #50 / run `34863968744`：39 passed，1 failed。唯一失败是新增断言“网络页不应再包含代理订阅”，当时页面仍显示该模块，故为预期 Red。

### 10.2 完整 Green 候选

代码候选 commit：`d980a14a587342d272f5ba17987d7dce1afcc4f1`。
Portal CI #73 / run `34883024079` 全绿：

| Job | 结果 |
| --- | --- |
| Static checks | success |
| Contract fmt/test/clippy | success |
| Frontend build + web unit | success |
| Things native fmt/test/clippy | success |
| Playwright E2E | success，**49 passed (3.0m)** |

Playwright 覆盖的关键新证据包括：

- Study 页面停留时不请求 device-policies、subscription、Tailscale peers，也不 POST proxy delays。
- Network 页面不请求设备、订阅、peers、proxy delays。
- Proxy subscription GET 失败只显示订阅模块错误，LAN TUN/本机系统代理仍可操作。
- Proxy 普通空闲轮询不会 POST `/control/proxy/delays` 或 `/control/proxy/subscription/refresh`。
- 设备 GET 人为延迟时，同一资源最多一个请求在途。
- hidden 期间普通设备 polling 暂停；恢复 visible 后立即刷新。
- 设备 policy 409 后本地编辑草稿仍保留。
- device write 成功但 readback 失败显示“已保存，状态刷新失败”。
- protected resource 401 后受保护缓存清除但 `#/devices` 目标保留，重新登录后恢复当前页。
- typed routes 支持 legacy activity canonicalization、设备深链接、筛选 query、浏览器前进/后退、reload、登录保留。
- Tailscale mutation 人为延迟 3 秒时，mutation 在途期间状态 GET 计数不增加；完成后立即出现新的状态 GET。

此前实施中间候选 `15873bc0a1fb619281999bcecf668f9dd1d31a8a` 也曾通过完整 Portal CI `34878084264`，Playwright 47 passed；最终候选在此基础上增加边界验证并清理过渡源码。

## 11. 实施阶段完成情况

### 阶段 A：资源模型与错误隔离

- [x] 用 per-resource state 替代 `fetch_settings_data()` 全成功屏障。
- [x] 网络、pending、订阅、设备、Tailscale、peers、status/panel/apps 分别维护 loading/error/last-success/request generation。
- [x] 401/auth epoch 清理受保护缓存并阻止旧响应回填。
- [x] 每个失败模块有独立 retry，旧成功数据在刷新失败时保留。

### 阶段 B：页面订阅与统一刷新

- [x] 当前页决定资源依赖。
- [x] 一个资源只允许一个读取请求在途。
- [x] hidden 降频/暂停，visible/focus/online 立即刷新当前页。
- [x] 配置类数据不参与 2 秒无意义全局轮询。
- [x] Study 请求隔离已由 E2E request listener 验证。
- [x] 普通 polling 不触发 delay/subscription mutation。

### 阶段 C：设备工作流

- [x] Activity 一级页升级为 Devices。
- [x] 合并 discovered/configured-offline/activity-only 设备。
- [x] 详情显示 policy、在线状态、Mihomo 可观测流量与连接。
- [x] label/policy 可在详情修改，手工 MAC 能力保留。
- [x] 409 保留草稿并刷新服务器状态。
- [x] write success/readback failure/effective runtime 状态分别表达。
- [x] 详情路由、筛选、Escape、焦点/滚动恢复已实现。

### 阶段 D：页面职责、路由与清理

- [x] Network 只保留网络职责。
- [x] Subscription 迁到 Proxy。
- [x] Device policy/activity 迁到 Devices。
- [x] Tailscale 独立读取与 mutation 调度。
- [x] hash route 刷新、前进/后退、登录后恢复已实现。
- [x] 旧 `app_v2/network_v2/activity` 双轨源码已清理。
- [x] PR 说明已更新为实施结果、自动化证据和目标板剩余项。

## 12. 目标板验收清单（尚未执行）

以下必须在真实 RK3568 上由用户执行；当前 PR/CI 不宣称这些项目已通过：

- [ ] 通过实际 LAN 地址打开门户，确认首页、网络、代理、设备、Tailscale、Camera、Study 均正常。
- [ ] 从 Tailscale 地址打开管理门户，确认远程限制和自停保护仍正确。
- [ ] 手机浏览器停到后台再恢复，确认当前页立即刷新且后台请求频率符合预期。
- [ ] 使用浏览器 Network/板端日志观察真实请求负载，确认 Study/Network/Proxy/Devices/Tailscale 页面不产生计划外请求。
- [ ] 实际修改设备直连/代理策略，确认 fresh snapshot `effective` 与数据面流向一致。
- [ ] 在实际设备离线、重新关联、随机 MAC 变化时确认 Devices 展示语义。
- [ ] 验证 Camera 离页释放、麦克风权限和 Study/PiP 无回归。

## 13. 完成定义

代码侧完成定义已经满足：

- [x] 一个资源失败不会拖垮无关资源或无关操作。
- [x] 页面只订阅自己需要的数据，Study 不再承受设备/订阅/peers/测速请求。
- [x] hidden/visible/focus/online 调度、单请求在途与错误退避已落地。
- [x] mutation 仅影响相关资源，旧代次/旧认证请求不能覆盖新状态。
- [x] 设备列表、详情、策略、Mihomo activity 与 typed URL 工作流完整。
- [x] 保存结果和运行时生效状态不混淆。
- [x] Network/Proxy/Devices/Tailscale 页面职责清晰，过渡源码已清理。
- [x] GitHub Actions 完整代码候选全绿，Playwright 49/49 通过。
- [x] PR 说明记录自动化证据及目标板未验收边界。

因此本计划当前结论为：**代码已实施，目标板验收待完成**。目标板清单通过后，才可把整个功能验收状态改为“目标板已验证/可合并”。
