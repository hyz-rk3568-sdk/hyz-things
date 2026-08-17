# Mihomo core、LAN TUN 与 Tailscale 本机显式代理双开关方案

## 状态

**2026-08-14：主机侧实现与自动验证已完成；RK3568 目标板部署、真实故障注入和验收矩阵仍待执行。**

当前实现已包含版本化双 feature desired、共享 Mihomo core、ownership-aware LAN TUN、固定 `tailscaled` Direct/Mihomo environment、core 退出后的 daemon-owned Direct 恢复、固定 `controlplane.tailscale.com:443` 的 3 秒有界 HTTP CONNECT 路径探测、两个 exact HTTP mutation、两个独立 Web 开关及桌面/360px Chromium E2E。尚未据此文档执行板端四组合、Mihomo `SIGKILL`、节点全失效、重启恢复和长时间稳定性验收，因此本文后半部分的目标板项目仍是待办，而不是已验证事实。

本文替代此前将 `tailscaled` 显式代理建模为独立 Tailscale 出站模式的方案。新的产品模型以 Mihomo core 为共享运行基础，将两个数据面能力拆成独立开关：

```text
Mihomo core 生命周期
    ↑
    ├── LAN TUN 开关
    └── Tailscale 本机显式代理开关
```

两个开关任意一个启用时，Mihomo core 必须运行；两个开关都关闭时，Mihomo core 停止，但订阅、provider、节点选择和其他 root-only userdata 配置继续保留。普通 NAT 在路由启用时始终是 LAN 的安全基线。浏览器始终不能直连 Mihomo Controller。

记录日期：2026-08-14。

相关文档：

- [`architecture.md`](architecture.md)
- [`soft-router-user-stories.md`](soft-router-user-stories.md)
- [`soft-router-tun.md`](soft-router-tun.md)
- [`soft-router-tailscale-plan.md`](soft-router-tailscale-plan.md)

## 1. 设计目标

当前 `ProxyMode::Disabled / Explicit / Tun` 把以下不同概念合并成了一个互斥枚举：

1. Mihomo core 是否运行；
2. LAN 是否进入 TUN；
3. LAN 客户端是否可以手工使用 mixed port；
4. 本机进程是否显式使用 Mihomo；
5. 普通 NAT 是否作为回退存在。

新的设计将它们重新拆分：

- LAN 只支持普通 NAT 或 TUN 透明代理，不再提供面向 LAN 客户端的手工显式代理模式；
- 本机不安装全局 `OUTPUT` TUN，只允许受控的本机进程显式连接固定本机代理入口；
- 第一阶段唯一使用本机显式代理的进程是 `tailscaled`；
- Mihomo core 是两个能力共享的内部运行依赖，不再作为用户选择的互斥“模式”；
- 两个数据面能力独立启停、独立观测、独立回滚；
- 任一能力失败不得破坏另一个已经确认 ready 的能力；
- Mihomo 停止后保留订阅配置，普通 NAT 继续可用；
- 浏览器只经过 `hyz-router` 的 typed HTTP API，不直连 Mihomo Controller、mixed port 或原始配置。

## 2. 术语与边界

### 2.1 Mihomo core

`/usr/bin/mihomo` 的 exact 进程、runtime config、TUN 接口、mixed port 和必要运行文件的统称。

Mihomo core 是否需要运行由两个 desired 开关派生：

```text
mihomo_required = lan_tun_enabled || tailscale_explicit_proxy_enabled
```

Mihomo core 不是第三个用户开关。用户只控制两个实际数据面能力。

### 2.2 LAN TUN

仅接管来自固定 LAN 的 IPv4 TCP/UDP：

```text
br-lan 192.168.8.0/24
  → HYZ_MIHOMO_PRE
  → fwmark 0x1000000/0x1000000
  → table 110
  → hyz-mihomo
```

它不安装本机 `OUTPUT` 入口，不接管 RK3568 本机进程，不改变 Tailscale-to-LAN 路径，也不把普通 NAT 从产品中删除。

### 2.3 Tailscale 本机显式代理

仅让 `tailscaled` 的 HTTP/HTTPS 出站显式连接固定本机 Mihomo mixed port：

```text
HTTP_PROXY=http://127.0.0.1:7890
HTTPS_PROXY=http://127.0.0.1:7890
```

第一阶段只针对：

- Tailscale 控制面 HTTPS；
- DERP TCP/TLS；
- `tailscaled` 内部遵循 HTTP proxy 环境的其他 HTTPS 请求。

它不代理：

- UDP 41641；
- STUN UDP；
- WireGuard direct UDP；
- 其他未显式配置代理的本机进程；
- 本机入站连接。

因此 Web 文案应使用 **“Tailscale 中继代理”**，不要使用容易被理解为全局系统代理的“本机代理”。

### 2.4 普通 NAT

普通 NAT 是路由启用时的 LAN 安全基线：

```text
LAN TUN 关闭或 fail-open
  → br-lan
  → ordinary router firewall/NAT
  → active WAN
```

Mihomo、Tailscale 或任一代理节点失败都不得删除已经确认 owned 的普通 NAT 基线。

## 3. 四种合法组合

| LAN TUN | Tailscale 中继代理 | Mihomo core | LAN 公网流量 | `tailscaled` HTTP/HTTPS |
| --- | --- | --- | --- | --- |
| 关 | 关 | 停止 | 普通 NAT | active WAN direct |
| 开 | 关 | 运行 | Mihomo TUN | active WAN direct |
| 关 | 开 | 运行 | 普通 NAT | 本机 mixed port |
| 开 | 开 | 运行 | Mihomo TUN | 本机 mixed port |

这四种组合都必须成为一等的 desired/observed 状态，不能再用单一 `ProxyMode` 猜测。

### 3.1 两个开关都关闭

```text
LAN TUN：Absent
Tailscale proxy environment：Direct
Mihomo core：Stopped
订阅与节点配置：Preserved
普通 NAT：Ready 或按现有 router 状态降级
```

### 3.2 只启用 LAN TUN

```text
LAN TUN：Ready
Tailscale proxy environment：Direct
Mihomo core：Running
```

这是现有 `Tun` 行为中与 LAN 数据面对应的部分。

### 3.3 只启用 Tailscale 中继代理

```text
LAN TUN：Absent
LAN：普通 NAT
Tailscale proxy environment：MihomoExplicit
Mihomo core：Running
```

该状态要求 Mihomo 只作为本机显式 HTTP CONNECT 出站代理运行，不安装 `br-lan` interception。

### 3.4 两个开关都启用

```text
LAN TUN：Ready
Tailscale proxy environment：MihomoExplicit
Mihomo core：Running
```

LAN 和 `tailscaled` 使用不同入口：

```text
LAN → hyz-mihomo TUN

tailscaled → 127.0.0.1:7890 → Mihomo outbound
```

两条路径共享 Mihomo core 和节点选择，但生命周期与 readiness 分开。

## 4. Web 设计

当前“Mihomo 模式”的三按钮：

```text
TUN
显式代理
停用
```

改为两个独立开关：

```text
代理设置

LAN 透明代理
[ 开 / 关 ]
通过 Mihomo TUN 接管来自 192.168.8.0/24 的下游流量；关闭后使用普通 NAT。

Tailscale 中继代理
[ 开 / 关 ]
只让 tailscaled 的 HTTP/HTTPS 和 DERP 连接使用当前 Mihomo 节点；UDP direct 仍直接探测。

当前代理节点
新加坡节点 A
```

继续显示固定安全提示：

```text
停用后保留订阅配置；普通 NAT 在路由启用时保持可用；浏览器不能直连 Mihomo Controller。
```

### 4.1 UI 状态

两个开关分别显示：

- configured：管理员持久 desired；
- effective：严格观测到的实际状态；
- busy：该能力正在切换；
- degraded：desired 已启用但依赖或回退未确认。

示例：

```text
LAN 透明代理：已启用
Tailscale 中继代理：已降级，当前已恢复 Direct
Mihomo core：运行中
```

Web 不得仅根据 Mihomo PID 存在就把两个开关都显示为 ready。

### 4.2 UI 安全边界

浏览器不能提交：

- mixed port；
- proxy URL；
- hostname 或 IP；
- `HTTP_PROXY`、`HTTPS_PROXY` 或其他环境变量；
- Mihomo provider 名；
- 原始节点名；
- DERP region、hostname 或 IP；
- 原始 Mihomo YAML；
- shell 命令、路径或 timeout；
- Mihomo Controller 地址或 secret。

节点选择继续使用现有受控 proxy-group 和 group-member 索引流程，不增加 Controller 透传。

## 5. Domain 模型

删除以互斥运行方式表达所有能力的：

```rust
enum ProxyMode {
    Disabled,
    Explicit,
    Tun,
}
```

改为正交 desired：

```rust
struct ProxyDesired {
    lan_tun_enabled: bool,
    tailscale_explicit_proxy_enabled: bool,
}
```

对应 observed 至少包含：

```text
persisted LAN TUN desired
persisted Tailscale explicit proxy desired
Mihomo exact process
runtime config validity
mixed-port readiness
TUN interface
LAN interception/firewall/policy route
Tailscale exact process environment
Mihomo fail-open watcher
ordinary router readiness
```

Mihomo core derived desired：

```rust
fn mihomo_required(desired: &ProxyDesired) -> bool {
    desired.lan_tun_enabled || desired.tailscale_explicit_proxy_enabled
}
```

不要引入公开的第三个 `mihomo_enabled` desired。否则会出现 core 已运行但两个数据面都关闭的无意义用户状态。

## 6. 持久状态

建议将单一 mode 文件替换为版本化、root-only typed 状态，例如：

```text
/userdata/hyz-router/mihomo/features.json
```

内容只包含固定布尔值和版本：

```json
{
  "version": 1,
  "lan_tun_enabled": true,
  "tailscale_explicit_proxy_enabled": false
}
```

要求：

- 目录 `0700`；
- 文件 `0600`；
- 有界读取；
- `deny_unknown_fields`；
- 临时文件、`fsync`、原子 rename；
- 不包含订阅 URL、节点名、provider、代理地址或凭据；
- desired 只有在对应 cutover 严格成功后才提交；
- unknown、损坏或不受支持版本不得猜测为 enabled。

### 6.1 旧 mode 迁移

现有持久值迁移为：

| 旧值 | `lan_tun_enabled` | `tailscale_explicit_proxy_enabled` | 原因 |
| --- | ---: | ---: | --- |
| `disabled` | false | false | 保持停用语义 |
| `explicit` | false | false | 旧值只表示 LAN 手工显式代理，不代表曾授权 `tailscaled` 使用代理 |
| `tun` | true | false | 保留现有 LAN TUN 行为，不自动改变 Tailscale 路径 |

迁移必须原子完成。遇到未知旧值时保持普通 NAT，不启动 Mihomo，也不删除旧文件，状态报告 Unknown，等待人工处理。

## 7. Mihomo runtime config

既然 LAN 不再提供手工显式代理，mixed port 应成为内部本机端口：

```yaml
mixed-port: 7890
allow-lan: false
bind-address: 127.0.0.1
```

LAN TUN 继续通过 `hyz-mihomo` TUN 设备工作，不依赖 LAN 客户端访问 mixed port。

运行时生成器必须：

1. 从 root-only source config 读取 provider、proxy-groups、rules 等受支持内容；
2. 删除调用方能够改变监听面和 TUN 所有权的顶层字段；
3. 固定追加受控 local-only mixed port；
4. 仅在 `lan_tun_enabled` 时追加受控 TUN 块；
5. 禁止 Controller、UI、LAN mixed-port、任意 bind-address 和自动路由；
6. 用 Mihomo `-t` 验证 runtime config；
7. 只将脱敏错误分类写入普通日志。

第一阶段若 loopback-only mixed port 与当前 TUN 实现存在兼容问题，可以保留 `192.168.8.1:7890`，但必须增加产品 owned INPUT 规则拒绝来自 `br-lan` 的 TCP/UDP 7890。最终边界仍应收紧为 loopback-only。

## 8. tailscaled 固定环境

当前 `LinuxTailscalePlatform` 使用 `env_clear()` 和固定 argv 启动 `tailscaled`。该约束继续保留。

### 8.1 Direct 环境

```text
PATH=/usr/sbin:/usr/bin:/sbin:/bin
LC_ALL=C
```

### 8.2 Mihomo 显式代理环境

```text
PATH=/usr/sbin:/usr/bin:/sbin:/bin
LC_ALL=C
HTTP_PROXY=http://127.0.0.1:7890
HTTPS_PROXY=http://127.0.0.1:7890
```

第一版不允许：

- `ALL_PROXY`；
- 小写代理变量；
- URL userinfo；
- 非 loopback host；
- 自定义 port；
- 浏览器输入环境；
- 从订阅或 provider 动态生成代理 URL。

进程 identity 应从 PID/start/exe/argv 扩展到 PID/start/exe/argv/environment。对 `/proc/<pid>/environ` 做有界精确解析；多余、重复、缺失、非 UTF-8 或无法读取的环境一律为 Unknown。

## 9. 生命周期动作顺序

### 9.1 开启 LAN TUN，Tailscale 代理保持关闭

```text
1. 获取共享网络生命周期锁
2. 确认 ordinary router forwarding ready
3. 若 Mihomo core 未运行，启动并验证 core/runtime config
4. 创建和验证 hyz-mihomo TUN
5. 安装 policy route、FORWARD 和 fail-open ownership
6. 最后安装 br-lan interception commit point
7. 严格复核 LAN TUN ready
8. 原子提交 lan_tun_enabled=true
9. 不重启 tailscaled
```

### 9.2 关闭 LAN TUN，Tailscale 代理保持开启

```text
1. 获取共享网络生命周期锁
2. 先删除 br-lan interception
3. 删除 LAN TUN policy route、FORWARD hook 和 owned marker
4. 保持 Mihomo core 和 loopback mixed port
5. 保持 tailscaled 代理环境
6. 确认普通 NAT ready
7. 原子提交 lan_tun_enabled=false
```

### 9.3 开启 Tailscale 中继代理，LAN TUN 保持关闭

```text
1. 确认 Tailscale desired access mode 与 node state
2. 若 Mihomo core 未运行，以无 LAN TUN 的 runtime config 启动
3. 验证 exact Mihomo process 和 127.0.0.1:7890
4. 安全停止 exact tailscaled，保留 node state
5. 以固定 proxy environment 启动 tailscaled
6. 恢复固定 preferences、route advertisement 和 management listener
7. 复核 Tailscale readiness 与 exact environment
8. 原子提交 tailscale_explicit_proxy_enabled=true
9. 不安装 br-lan interception
```

### 9.4 关闭 Tailscale 中继代理，LAN TUN 保持开启

```text
1. 安全停止带 proxy environment 的 exact tailscaled
2. 以 Direct environment 启动 tailscaled
3. 恢复 preferences、route advertisement 和 listener
4. 复核 Direct environment 与 Tailscale readiness
5. 原子提交 tailscale_explicit_proxy_enabled=false
6. 保持 Mihomo core、TUN 和 br-lan interception
```

### 9.5 两个开关都关闭

安全顺序：

```text
1. 如果 Tailscale 代理开启，先恢复 tailscaled Direct
2. 如果 LAN TUN 开启，先撤销 br-lan interception 并确认普通 NAT
3. 确认两个 effective capability 都 absent/direct
4. 停止 exact Mihomo core 和 fail-open watcher
5. 清理 runtime-only config、PID、socket 和 owned marker
6. 保留 userdata 订阅、provider 来源、节点选择和 feature desired
7. 原子提交两个 desired=false
```

不能先停止 Mihomo，再让仍带 `HTTP_PROXY` 的 `tailscaled` 失去代理入口。

## 10. 独立失败与回滚

### 10.1 LAN TUN 切换失败

如果 Tailscale 中继代理已 ready：

```text
LAN → 回退普通 NAT
Mihomo core → 保持运行
mixed port → 保持 ready
tailscaled → 继续使用显式代理
```

LAN TUN 失败不得停止仍被 `tailscaled` 使用的 Mihomo core。

### 10.2 Tailscale 代理切换失败

如果 LAN TUN 已 ready：

```text
LAN TUN → 保持运行
Mihomo core → 保持运行
tailscaled → 尝试恢复 Direct
```

Tailscale 代理失败不得撤销 LAN TUN。

### 10.3 Mihomo core 退出

现有 fail-open watcher 继续优先撤销 LAN interception，使 LAN 回到普通 NAT。

对 `tailscaled`：

1. daemon 观察到 mixed port 和 exact Mihomo process 不 ready；
2. 若 desired Tailscale proxy 为 true，执行有界 Direct 重启；
3. 成功时 effective Tailscale proxy 为 direct-restored/degraded；
4. 失败时只将 Tailscale 标记 degraded；
5. ordinary router 和 LAN 管理面不受影响。

第一阶段 watcher 不直接管理 Tailscale。它保持最小权限，只撤销 Mihomo TUN；Tailscale Direct 恢复由正常 application reconcile 负责。

### 10.4 Mihomo 进程存活但节点全部失效

Mihomo process alive 不代表 Tailscale 代理 ready。第一阶段至少需要：

- loopback mixed-port TCP probe；
- 有界的实际代理探测或 Tailscale DERP 状态观察；
- 明确的失败分类；
- 用户可手动关闭 Tailscale 中继代理并恢复 Direct。

自动基于延迟切换代理节点不属于第一阶段。不能让浏览器提交测试 URL 或 timeout。

## 11. Application 与 ports

Application 应围绕两个 use case，而不是一个三值 mode：

```text
set_lan_tun_enabled(bool)
set_tailscale_explicit_proxy_enabled(bool)
```

ports 至少提供 typed 能力：

- 观察和启动/停止 Mihomo core；
- 安装/撤销 LAN TUN 数据面；
- 探测固定 loopback mixed port；
- 观察 `tailscaled` exact environment；
- 以 Direct 或固定 Mihomo environment 重启 `tailscaled`；
- 读取/提交版本化 feature desired；
- 获取共享生命周期锁。

Application 不接收：

- proxy URL；
- mixed port；
- executable；
- argv；
- environment map；
- provider 或节点字符串；
- Mihomo YAML。

具体 Linux 路径、`Command`、`/proc` 和固定环境只属于 outbound adapter。

## 12. HTTP API

建议废弃当前单一：

```text
POST /api/v1/control/proxy/mode
```

改为两个 exact typed mutation：

```text
POST /api/v1/control/proxy/lan-tun
POST /api/v1/control/proxy/tailscale
```

请求体均只允许：

```json
{"enabled":true}
```

或者使用一个固定组合 DTO：

```text
POST /api/v1/control/proxy/features
```

```json
{
  "lan_tun_enabled": true,
  "tailscale_explicit_proxy_enabled": false
}
```

如果使用组合 DTO，application 仍必须按依赖顺序分别 reconcile 两个能力，不能把它们当成不可分割事务。推荐两个 exact mutation，便于独立 busy、错误和回滚。

所有接口继续要求：

- 管理员认证；
- same-origin；
- CSRF；
- JSON body limit；
- `deny_unknown_fields`；
- 固定 method/path；
- 不回显秘密；
- 不向 Tailscale listener 暴露超出现有管理权限的能力。

## 13. 状态模型

代理状态应分层展示：

```text
Mihomo core
  configured required: yes/no
  process: running/stopped/unknown
  runtime config: ready/not-ready/unknown
  mixed port: ready/absent/unknown

LAN TUN
  desired: enabled/disabled/unknown
  effective: ready/ordinary-nat/not-confirmed

Tailscale 中继代理
  desired: enabled/disabled/unknown
  environment: mihomo/direct/unknown
  connection: direct/peer-relay/derp(region)/unknown
  fallback: not-needed/direct-restored/not-confirmed
```

匿名状态只显示必要健康摘要。管理员设置页可以显示两个开关和脱敏错误，不显示完整 netmap、代理 URL、provider、节点凭据或登录 token。

## 14. 测试驱动实现要求

### 14.1 Domain 与 planner

使用纯状态和 fake ports 覆盖全部四种组合：

1. `00 → 10`：只启动 LAN TUN，不重启 tailscaled。
2. `00 → 01`：只启动 Mihomo mixed port 并重启 tailscaled，不安装 LAN hook。
3. `10 → 11`：保持 TUN，只重启 tailscaled 为代理环境。
4. `01 → 11`：保持 tailscaled 代理，只安装 LAN TUN。
5. `11 → 10`：先恢复 tailscaled Direct，保持 LAN TUN 和 Mihomo。
6. `11 → 01`：只撤销 LAN TUN，保持 Mihomo 和 tailscaled 代理。
7. `10 → 00`：撤销 TUN、确认普通 NAT、停止 Mihomo。
8. `01 → 00`：恢复 tailscaled Direct、停止 Mihomo。
9. `11 → 00`：严格按先 Direct、再 ordinary NAT、最后 stop core 的顺序。
10. LAN TUN 失败不影响已 ready 的 Tailscale 代理。
11. Tailscale 代理失败不影响已 ready 的 LAN TUN。
12. Mihomo unknown/foreign/stale 时拒绝接管或删除。
13. tailscaled environment unknown 时拒绝误报代理 ready。
14. ordinary router not ready 时不允许提交 LAN TUN ready，但可按现有安全边界处理 RouterOnly Tailscale。
15. 操作中途失败执行最小逆序补偿，不删除 foreign 状态。

### 14.2 Adapter

覆盖：

- fixed executable 与 argv；
- `env_clear()`；
- 仅允许 Direct 和固定 loopback proxy 两组环境；
- exact PID/start/exe/argv/environment；
- local-only mixed port；
- 无 `sh -c`；
- 无浏览器提供命令、URL、端口或 provider；
- TUN hook 只匹配 `-i br-lan -s 192.168.8.0/24`；
- 不安装本机 `OUTPUT` TUN；
- Mihomo core 无 LAN TUN 时不遗留 interception、policy route 或 TUN firewall；
- 两个 capability 共用 core 时，关闭一个不会误停 core；
- 最后一个 capability 关闭后才停止 core。

### 14.3 HTTP 与 Web

验证：

- 当前三值 mode 控件被两个独立开关替代；
- 两个开关有独立 busy 和错误；
- 一侧失败不会错误翻转另一侧 UI；
- 空状态、unknown、degraded、direct-restored 均有明确文案；
- 节点选择继续受控；
- 浏览器不能连接 Controller 或提交 raw config；
- desktop 和 mobile 布局均可操作；
- 所有共享状态页面显示一致。

## 15. 目标板验收矩阵

| LAN TUN | Tailscale 代理 | 需要验证 |
| --- | --- | --- |
| 关 | 关 | Mihomo 无进程，LAN 普通 NAT，Tailscale direct/DERP 正常，配置保留 |
| 开 | 关 | LAN TCP/UDP 命中 TUN，tailscaled 无 proxy environment |
| 关 | 开 | LAN 普通 NAT，无 TUN hook，tailscaled 命中 mixed port |
| 开 | 开 | LAN TUN 与 tailscaled 显式代理同时工作，无递归 |

每种组合至少执行：

```sh
hyz-router status --json
ip rule
ip route show table 110
iptables-save
ps
ss -lntup
tailscale --socket=/run/hyz-tailscale/tailscaled.sock status
tailscale --socket=/run/hyz-tailscale/tailscaled.sock netcheck
tailscale --socket=/run/hyz-tailscale/tailscaled.sock ping --verbose --c 20 hyz
```

还需验证：

1. 四种组合全部双向切换。
2. 完整重启后恢复 desired 与 effective。
3. `hyz-router restart` 不产生重复进程、规则或 hook。
4. Mihomo `SIGKILL` 后 LAN 回退普通 NAT。
5. Mihomo `SIGKILL` 后 tailscaled 有界恢复 Direct。
6. tailscaled 退出不破坏 LAN TUN。
7. 所有代理节点失效时不误报 Tailscale 代理 ready。
8. WAN DHCP 变化与 `wlan0` 断开恢复。
9. Tailscale direct 成功时仍优先 direct。
10. RouterOnly、LanSubnetAccess 和远程管理 listener 无回归。
11. 浏览器控制在桌面和移动视口完成端到端验证。
12. 日志不包含订阅、节点凭据、管理员密码或 Tailscale 登录 token。

## 16. 实施顺序

1. 用测试定义两个独立 desired、observed 和四种组合。
2. 增加旧 `ProxyMode` 持久状态迁移测试。
3. 重构 domain 与 application planner，不先修改 Web。
4. 扩展 Mihomo runtime config，固定 loopback mixed port 并允许无 TUN运行。
5. 扩展 Tailscale adapter 的 fixed environment 与 exact environment observation。
6. 实现两个能力独立 cutover、rollback 和 shared-core 引用语义。
7. 增加 root-only control 操作与状态。
8. 替换 HTTP DTO 和 Web 三按钮为两个独立开关。
9. 运行静态检查和主机自动测试。
10. 用户明确授权后运行 Rust/frontend、Buildroot、rootfs 和 OTA 构建。
11. 通过 USB ADB 安装 recovery-free OTA。
12. 按四组合、故障注入、重启和浏览器矩阵完成板端验收。

## 17. 最终产品语义

最终对用户只呈现两个独立能力：

```text
LAN 透明代理
  开：LAN 进入 Mihomo TUN
  关：LAN 使用普通 NAT

Tailscale 中继代理
  开：tailscaled HTTP/HTTPS 使用本机 Mihomo
  关：tailscaled 使用 active WAN direct
```

内部生命周期为：

```text
Mihomo core 生命周期
    ↑
    ├── LAN TUN 开关
    └── Tailscale 本机显式代理开关
```

共享 core 不等于共享 readiness。两个能力独立控制、独立观测、独立失败；任一开启就保留 Mihomo core，全部关闭才停止 core。停止后保留订阅与节点配置，普通 NAT 继续作为 LAN 安全基线，浏览器始终不能直连 Mihomo Controller。
