# hyz-things Mihomo TUN 智能分流优化计划

## 状态

**代码已实施，目标板验收待完成。**

创建日期：2026-09-11。
最后更新日期：2026-09-11。

本文用于优化 `hyz-things` 当前 Mihomo LAN TUN 的流量分流策略。

当前 LAN TUN 已能够将 `br-lan` 下游 IPv4 TCP/UDP 透明送入 Mihomo，并具备受控 TUN、policy routing、资源 ownership、watcher 和 ordinary NAT fail-open。现阶段缺少的是稳定的产品级代理策略：机场订阅仅提供节点后，`hyz-things` 尚未统一生成自己的 proxy group，以及面向国内流量的 `GEOSITE,cn + GEOIP,CN` DIRECT 规则。

本次只实现：

```text
subscription proxies-only
        ↓
HYZ-PROXY
HYZ-AUTO
        ↓
GEOSITE,cn,DIRECT
GEOIP,CN,DIRECT
        ↓
MATCH,HYZ-PROXY
        ↓
Web 正确展示 selector / url-test
        ↓
板测 CN DIRECT / foreign PROXY
```

不扩大到 Mihomo DNS、sniffer、fake-ip、dns-hijack 或 IPv6。

## 1. 目标

本次计划完成以下事项：

- 继续保持机场订阅只提供 `proxies`；
- 不接受机场 `rules`、`proxy-groups`、DNS 或其他完整 Mihomo 配置；
- 由 `hyz-things` 自动根据当前订阅节点生成固定产品代理组；
- 建立 `HYZ-PROXY` 手动选择组；
- 建立 `HYZ-AUTO` 自动测速组；
- 固定产品规则为：
  - 中国大陆域名 → `DIRECT`；
  - 中国大陆 IPv4 → `DIRECT`；
  - 其他流量 → `HYZ-PROXY`；
- 用户从 Web 选择节点时，只改变“需要代理的流量”的出口；
- Web 正确区分 `Selector` 与 `UrlTest` group；
- 订阅更新后自动同步 proxy group 节点集合；
- 对产品托管的本地订阅 provider 禁用 Mihomo 自启动健康检查；`HYZ-AUTO` 通过 `use: [subscription]` 复用该 provider，不再由 inline `proxies` 配置隐式创建另一份后台健康检查；节点测速只通过有界的产品请求执行；
- 保持现有 LAN TUN lifecycle、ordinary NAT fail-open、device policy 和本机系统代理边界；
- 在目标板验证真实 CN DIRECT 和 foreign PROXY。

## 2. 非目标

本次不实现：

- Mihomo DNS；
- dnsmasq → Mihomo DNS；
- `dns-hijack`；
- fake-ip；
- redir-host；
- sniffer；
- DoH/DoQ 管理；
- IPv6 TUN；
- Netflix、ChatGPT、Google 等业务级分流；
- 导入机场 `rules`；
- 导入机场 `proxy-groups`；
- 导入机场 DNS；
- 多机场合并；
- 自动根据节点名称过滤“剩余流量”“套餐到期”等信息节点；
- 本机 `OUTPUT` 透明代理；
- 节点全部失效时自动切换 ordinary NAT。

这些能力如有需求应使用独立 plan。

## 3. 固定架构边界

机场订阅继续保持：

```text
remote subscription
       │
       ▼
 validate / parse
       │
       ▼
   proxies only
```

当前订阅 parser 的安全边界不变：远端 `proxy-groups`、`rules` 等策略字段不进入产品配置。

目标变为：

```text
机场
 │
 │ proxies
 ▼
hyz-things
 ├── HYZ-PROXY
 ├── HYZ-AUTO
 └── rules
       │
       ▼
     Mihomo
```

职责固定为：

```text
机场：
提供节点

用户：
选择代理出口 / AUTO

hyz-things：
决定 DIRECT / PROXY

Mihomo：
执行规则和节点连接
```

## 4. 保持现有 LAN TUN 数据面

本次不重构当前数据面：

```text
br-lan 192.168.8.0/24
        ↓
HYZ_MIHOMO_PRE
        ↓
fwmark 0x1000000/0x1000000
        ↓
table 110
        ↓
hyz-mihomo
```

继续保持：

- IPv4 only；
- LAN downstream only；
- TCP/UDP only；
- 无本机 `OUTPUT` interception；
- 私网和设备 Direct MAC 可在进入 Mihomo 前 bypass；
- Mihomo `auto-route: false`；
- Mihomo `auto-redirect: false`；
- TUN ownership token；
- policy route ownership；
- firewall ownership；
- core crash 后 ordinary NAT fail-open。

本次不改变受控 TUN block 的 DNS 行为。

## 5. 产品自有 Proxy Group

### 5.1 HYZ-PROXY

建立固定 selector：

```yaml
proxy-groups:
  - name: HYZ-PROXY
    type: select
    proxies:
      - HYZ-AUTO
      - <subscription node 1>
      - <subscription node 2>
      - <subscription node 3>
```

`HYZ-PROXY` 是产品的主代理出口。

用户在 Web 中可以选择：

```text
HYZ-AUTO
日本节点
新加坡节点
香港节点
...
```

用户选择只改变：

```text
需要代理的流量
      ↓
HYZ-PROXY
      ↓
chosen proxy
```

不会改变 `DIRECT` 流量。

### 5.2 HYZ-AUTO

建立固定 `url-test`，复用产品托管的本地 `subscription` provider：

```yaml
proxy-providers:
  subscription:
    type: file
    path: ./providers/subscription.yaml
    health-check:
      enable: false

proxy-groups:
  - name: HYZ-AUTO
    type: url-test
    use:
      - subscription
```

`HYZ-AUTO` 不再同时写入 inline `proxies`、`url` 和 `interval`。在 Mihomo v1.19.29 中，`url-test` 使用 inline `proxies` 会隐式创建兼容 provider，并在启动时立即对组内节点执行健康检查；复用已关闭自动健康检查的产品 provider，可以避免同一批订阅节点被重复并发探测。

`HYZ-AUTO` 的节点集合仍由当前订阅同步，产品 panel 只在用户请求测速或刷新时发起有界探测。单节点探测窗口为 10 秒，Controller 传输超时为 30 秒。

具体字段必须通过当前固定 Mihomo 版本的：

```sh
mihomo -t
```

校验。

`HYZ-AUTO`：

- 不允许用户手工选择内部节点；
- 根据产品发起的测速结果自动选择当前合适节点；
- Web 只展示其当前选择、alive 和 delay。

当前 router panel 已能识别 `Selector`、`URLTest`、`Fallback`、`LoadBalance`、`Relay` 等 Mihomo group，并且只有 `Selector` 应当允许用户直接选择。

## 6. Group 名称 ownership

以下名称由产品保留：

```text
HYZ-PROXY
HYZ-AUTO
```

如果机场节点名称与产品保留名冲突：

```text
candidate invalid
       ↓
拒绝本次订阅更新
       ↓
保留上一份有效配置
```

不得：

- 静默覆盖；
- 自动改机场节点名称；
- 让节点与 group 同名进入 Mihomo runtime。

这样可以保持 Web、Controller、subscription 和 runtime 中节点 identity 一致。

## 7. 产品自有 Rules

第一阶段同时使用域名和 IP 两层国内分流：

```yaml
mode: rule

rules:
  - GEOSITE,cn,DIRECT
  - GEOIP,CN,DIRECT,no-resolve
  - MATCH,HYZ-PROXY
```

最终语法、大小写和参数必须以当前产品固定 Mihomo 版本执行 `mihomo -t` 的结果为准。

规则顺序必须固定：

```text
GEOSITE,cn,DIRECT
        ↓
GEOIP,CN,DIRECT
        ↓
MATCH,HYZ-PROXY
```

不得把 `MATCH` 放在国内规则之前。

### 7.1 GEOSITE,cn

`GEOSITE,cn` 根据目标域名所属规则集判断国内流量。

它主要解决仅依赖目标 IP 时无法准确判断网站属性的问题，例如国内网站使用境外 CDN、跨区域云服务等情况。

命中后：

```text
domain classified as CN
        ↓
      DIRECT
```

本阶段不引入 sniffer，因此 `GEOSITE,cn` 只能使用 Mihomo 当前实际可获得的域名信息。不能把它描述为所有连接都必然可进行域名判断。

### 7.2 GEOIP,CN

当域名规则未命中或流量主要按目标 IPv4 判断时，继续使用：

```text
destination IPv4 classified as CN
        ↓
      DIRECT
```

因此两层策略为：

```text
域名属于 CN
   ↓ yes
DIRECT

   ↓ no / unavailable
IP 属于 CN
   ↓ yes
DIRECT

   ↓ no
HYZ-PROXY
```

## 8. GeoSite / GeoIP 数据

实施前必须确认当前固定 Mihomo 版本与 rootfs 中存在可用、匹配的 GeoSite 和 GeoIP 数据。

不能把：

```text
设备启动时在线下载 geodata
```

作为正常工作前提。

如果当前 image 已经具备稳定资产，则直接复用。

如果缺失，应在本计划范围内只补足 `GEOSITE,cn` 与 `GEOIP,CN` 所必需的固定数据资产：

```text
fixed source
fixed version
fixed SHA-256
fixed rootfs path
```

不得借本次改动引入大量 Netflix、ChatGPT、Telegram 等业务规则。

如果 GeoSite / GeoIP 数据缺失、损坏或与当前 Mihomo 不兼容：

```text
candidate validation / readiness fails
          ↓
不得提交新的 managed routing runtime
```

不能静默退化成全量 `MATCH,HYZ-PROXY` 后仍报告正常。

## 9. Runtime 配置生成

当前订阅刷新继续只把经过验证的 `proxies` 作为远端输入。

本次在 runtime/candidate composition 中增加产品策略：

```text
validated proxies
       ↓
generate HYZ-AUTO
       ↓
generate HYZ-PROXY
       ↓
generate managed GEOSITE/GEOIP rules
       ↓
controlled TUN/listener/controller
       ↓
mihomo -t
```

必须满足：

```text
subscription refresh
    ↓
节点新增/删除
    ↓
HYZ-PROXY 自动更新
HYZ-AUTO 自动更新
```

用户不需要手工编辑 YAML。

## 10. Managed 字段

至少以下字段由本次开始明确归产品所有：

```text
mode
proxy-groups
rules
```

机场或持久 source 中同名字段不能覆盖产品策略。

现有受控字段继续保持：

```text
tun
listener
mixed-port
allow-lan
bind-address
authentication
controller
secret
```

推荐逐步将 runtime 生成逻辑收敛为：

```text
trusted node input
       +
product policy
       +
controlled runtime fields
       ↓
runtime YAML
```

而不是允许远端策略参与 merge。

## 11. Web 行为

当前 Web / contract 已经能够表达：

```text
ProxyGroup {
    name,
    kind,
    selectable,
    selected,
    options
}
```

且切换请求包含：

```text
group
proxy
```

因此 wire model 不需要因为本次增加 AUTO 而重做。

### 11.1 HYZ-PROXY

展示：

```text
代理出口

[ 自动选择 / 日本 / 新加坡 / 香港 ▼ ]
```

可以操作。

### 11.2 HYZ-AUTO

展示：

```text
自动选择

当前节点：日本
延迟：82 ms
```

只读。

不能将 `HYZ-AUTO` 显示成可操作下拉框。

## 12. 当前代理节点展示

增加两个产品 group 后，页面顶部“当前代理节点”不能继续依赖找到任意第一个存在 `selected` 的 group。

应明确以：

```text
HYZ-PROXY
```

作为产品主 group。

如果：

```text
HYZ-PROXY → 新加坡节点
```

显示：

```text
当前代理节点
新加坡节点
```

如果：

```text
HYZ-PROXY → HYZ-AUTO
HYZ-AUTO → 日本节点
```

建议显示：

```text
当前代理策略
自动选择

当前实际节点
日本节点
```

不得把其他任意 group 的 `selected` 当成产品主出口。

## 13. 用户选择节点的固定语义

这是本阶段最重要的外部行为 contract。

例如用户选择：

```text
新加坡-优化2-GPT
```

实际表示：

```text
HYZ-PROXY
    ↓
新加坡-优化2-GPT
```

访问命中 `GEOSITE,cn` 或 `GEOIP,CN` 的国内流量：

```text
LAN
 ↓
TUN
 ↓
GEOSITE/GEOIP CN
 ↓
DIRECT
```

不会使用新加坡节点。

访问未命中的 foreign 流量：

```text
LAN
 ↓
TUN
 ↓
MATCH
 ↓
HYZ-PROXY
 ↓
新加坡-优化2-GPT
```

因此节点选择的产品语义是：

> 需要代理的流量使用哪个出口。

不是：

> 所有流量都通过这个节点。

## 14. Device Policy

保持当前 device policy：

```text
Direct
Proxy
```

其中 `Direct` 继续在 TUN mangle chain 中通过 MAC `RETURN`，完全不进入 Mihomo。

因此：

```text
device Direct
     ↓
ordinary NAT
```

而：

```text
device Proxy
     ↓
进入 Mihomo policy
     ├── CN domain/IP → DIRECT
     └── foreign → HYZ-PROXY
```

当前 `Proxy` 更准确的语义是：

> 使用 Mihomo TUN 分流策略。

不是：

> 强制所有流量使用机场。

本次不新增 `Force Proxy` device policy。

## 15. 本机系统代理

本机系统代理继续使用：

```text
127.0.0.1:7890
```

并与 LAN TUN 共享 Mihomo core。

因为 rules 是 Mihomo runtime 全局策略，所以显式代理请求也会使用：

```text
GEOSITE,cn → DIRECT
GEOIP,CN → DIRECT
MATCH → HYZ-PROXY
```

本阶段接受该行为。

如果未来要求：

```text
LAN TUN → smart split
local proxy → always proxy
```

应另开独立计划，不在本次实现。

## 16. Subscription Refresh

正常流程保持：

```text
HTTPS fetch
   ↓
validation
   ↓
proxies only
   ↓
candidate
   ↓
mihomo -t
   ↓
atomic publish
```

本次增加：

```text
proxies
   ↓
managed groups
   ↓
managed rules
   ↓
candidate validation
```

更新失败必须保留上一份正常工作的 source/runtime。

不能因为新机场订阅出现：

```text
节点重名
无有效节点
group collision
Mihomo validation failure
```

而破坏当前正在工作的代理。

## 17. TDD 与实施顺序

遵循仓库 Red-Green-Refactor 规则。

建议顺序：

```text
baseline
   ↓
Red: subscription remains proxies-only
   ↓
GeoSite / GeoIP asset audit
   ↓
managed group generator
   ↓
HYZ-PROXY
   ↓
HYZ-AUTO
   ↓
group collision validation
   ↓
managed GEOSITE + GEOIP rules
   ↓
candidate validation
   ↓
subscription refresh tests
   ↓
panel group semantics
   ↓
Web selector/url-test UI
   ↓
host regression
   ↓
target-board validation
```

不能一次同时修改 subscription、runtime、panel 和 Web 再统一补测试。

## 18. Host 测试

至少覆盖以下行为：

```text
subscription strips remote rules
subscription strips remote proxy-groups

HYZ-PROXY includes HYZ-AUTO
HYZ-PROXY includes all current subscription nodes
HYZ-AUTO reuses the managed subscription provider
managed subscription provider startup health checks are disabled
HYZ-AUTO does not add a second inline health-check provider
removed subscription node disappears from both groups
reserved group-name collision is rejected

GEOSITE,cn appears before GEOIP,CN
GEOIP,CN appears before MATCH
MATCH always targets HYZ-PROXY

Selector is selectable
UrlTest is not selectable
selecting HYZ-PROXY node sends group + proxy

subscription refresh preserves last good config on invalid candidate
LAN TUN desired state is unchanged by node selection
local-system desired state is unchanged by node selection
```

测试必须通过 domain functions、application use cases、ports/fakes、ControlHandler、PanelPlatformPort fake、HTTP/Playwright seam 驱动。

普通 host 测试不得执行真实：

```text
iptables
ip
TUN
WAN access
real provider
```

## 19. Web E2E

Proxy 页面至少验证：

```text
HYZ-PROXY 可手动选择
节点列表包含当前订阅节点
HYZ-AUTO 是 HYZ-PROXY 的一个选项
HYZ-AUTO 本身不可手动选择内部节点
HYZ-AUTO 显示当前实际节点
UrlTest 显示为“自动测速”
用户选择实际节点后页面显示该节点
用户选择 HYZ-AUTO 后显示自动策略
节点切换请求仍包含 group + proxy
LAN TUN toggle 不受 group 修改影响
本机系统代理 toggle 不受 group 修改影响
```

继续优先使用 role、accessible name 和稳定产品文案作为 E2E selector，不依赖 Tailwind class 或 DOM hierarchy。

## 20. 目标板验收

最终必须使用真实 LAN client 验证。

至少包括：

1. TUN disabled 时 ordinary NAT 正常；
2. TUN enabled 后手机无需配置 HTTP proxy；
3. 一个确认属于国内域名集合的目标命中 `GEOSITE,cn,DIRECT`；
4. 一个确认属于 CN IPv4 的目标命中 `GEOIP,CN,DIRECT`；
5. 一个确认属于 foreign 的目标命中 `HYZ-PROXY`；
6. 用户选择节点 A，新的 foreign connection 使用节点 A；
7. 切换到节点 B，新的 foreign connection 使用节点 B；
8. 切换节点前后，CN connection 始终 `DIRECT`；
9. 选择 `HYZ-AUTO` 后 foreign traffic 使用 AUTO 当前实际节点；
10. `HYZ-AUTO` 能完成真实 delay test；
11. subscription refresh 后新增节点进入 group；
12. subscription refresh 后删除节点退出 group；
13. 机场自身 `rules` 不影响最终分流；
14. Direct device 继续绕过 TUN；
15. Mihomo core `SIGKILL` 后 ordinary NAT fail-open 不回归；
16. router restart 后 TUN ownership/order 不回归；
17. reboot 后 desired TUN 状态正常恢复；
18. 本机无 `OUTPUT` interception；
19. Tailscale 行为不变。

板测证据重点记录：

```text
Mihomo rule hit
using DIRECT
using HYZ-PROXY / actual proxy
group current selection
delay
TUN packet counters
fail-open state
```

不得记录订阅 URL/token、proxy password、节点 credential 或 controller secret。

## 21. Host 验证

按静态检查优先：

```sh
git diff --check
make check-static
```

Rust 相关测试按实际改动范围执行：

```sh
cargo fmt \
  --manifest-path apps/rust/router/Cargo.toml \
  --all -- --check

cargo test \
  --locked \
  --manifest-path apps/rust/router/Cargo.toml

cargo clippy \
  --locked \
  --manifest-path apps/rust/router/Cargo.toml \
  --all-targets \
  -- -D warnings
```

修改 Web 后对应执行 things native tests、things frontend 和 things Playwright E2E。

所有“未运行”的项目必须明确写为未运行，不得推断为通过。

## 22. 提交边界

建议保持小而独立的提交，例如：

```text
test(proxy): capture managed split-routing contract
feat(proxy): generate hyz proxy groups
feat(proxy): add cn geosite and geoip routing policy
test(subscription): cover managed groups on refresh
feat(web): distinguish manual and automatic proxy groups
test(e2e): cover proxy strategy selection
docs(proxy): document tun split-routing behavior
```

具体提交数量不是目标。

关键要求：

- group generation 和 Web 改动分开；
- rules 与订阅 parser 改动分开；
- 不夹带 DNS；
- 不夹带 sniffer；
- 不夹带 IPv6；
- 每个阶段都能够独立回滚。

## 23. 完成标准

只有满足以下条件，本文才能标记为完成：

- subscription 仍然只接受 `proxies`；
- 机场 `rules` 不进入 runtime；
- 机场 `proxy-groups` 不进入 runtime；
- `HYZ-PROXY` 自动生成；
- `HYZ-AUTO` 自动生成；
- groups 始终与当前订阅节点同步；
- `GEOSITE,cn,DIRECT` 生效；
- `GEOIP,CN,DIRECT` 生效；
- 最终 `MATCH` 固定进入 `HYZ-PROXY`；
- 用户节点选择只影响 proxy traffic；
- CN traffic 不因用户节点选择而进入机场；
- `HYZ-AUTO` 可工作；
- 分组测速使用独立且有界的 10 秒探测窗口和 30 秒 Controller 传输超时；
- Web 正确区分 selector 与 url-test；
- Direct device policy 不回归；
- LAN TUN 与本机系统代理独立开关不回归；
- existing ordinary NAT fail-open 不回归；
- host tests 和 Web E2E 通过；
- 真实板端 `GEOSITE CN DIRECT / GEOIP CN DIRECT / foreign PROXY` 已验证；
- 实际执行结果全部记录；
- 未执行项目明确标记。

## 24. 不在范围内

本计划完成后仍不代表以下能力完成：

```text
sniffer
Mihomo DNS
dnsmasq integration
dns-hijack
fake-ip
redir-host
DoH / DoQ interception
IPv6 TUN
业务级分流
节点全部不可用自动 fail-open
```

这些能力以后有明确需求再单独实施。
