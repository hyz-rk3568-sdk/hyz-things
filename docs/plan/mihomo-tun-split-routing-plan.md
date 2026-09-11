# hyz-things Mihomo TUN 智能分流优化计划

## 状态

**代码已实施，目标板验收待完成。**

创建日期：2026-09-11。
最后更新日期：2026-09-11。

本文定义 `hyz-things` 当前 Mihomo LAN TUN 的产品级分流策略。机场订阅只提供节点，产品只生成一个可手工选择的代理组 `HYZ-PROXY`，国内流量固定直连，其余流量进入用户当前选择的订阅节点。

本阶段固定模型：

```text
remote subscription
        ↓
   proxies only
        ↓
     HYZ-PROXY
      type: select
        ↓
所有订阅节点原样保留

rules:
GEOSITE,cn,DIRECT
GEOIP,CN,DIRECT,no-resolve
MATCH,HYZ-PROXY
```

不扩大到 Mihomo DNS、sniffer、fake-ip、dns-hijack、IPv6 或自动节点选择。

## 1. 目标

本次必须满足：

- 机场订阅继续只提供 `proxies`；
- 不接受机场 `rules`、`proxy-groups`、DNS 或其他完整 Mihomo 策略；
- 产品只生成一个固定代理组 `HYZ-PROXY`；
- `HYZ-PROXY` 类型固定为 `select`；
- `HYZ-PROXY.proxies` 与当前订阅节点列表一一对应并保持订阅顺序；
- 不根据节点名称过滤“官网”“剩余流量”“套餐到期”等订阅条目；
- 不生成 `HYZ-AUTO` 或其他产品自动选择组；
- 固定规则为：
  - 中国大陆域名 → `DIRECT`；
  - 中国大陆 IPv4 → `DIRECT`；
  - 其他流量 → `HYZ-PROXY`；
- 用户从 Web 选择节点时，只改变需要代理的流量出口；
- Web 顶部“当前代理节点”明确读取 `HYZ-PROXY.selected`；
- 订阅刷新后 `HYZ-PROXY` 自动同步当前订阅节点集合；
- 节点延迟刷新继续由产品通过受限 Controller 请求执行，不引入额外后台自动策略组；
- 保持现有 LAN TUN lifecycle、ordinary NAT fail-open、device policy 和本机系统代理边界；
- 最终由目标板验证真实 CN DIRECT 与 foreign PROXY。

## 2. 非目标

本阶段不实现：

- `HYZ-AUTO`、`url-test` 自动节点选择；
- 自动根据节点名称过滤信息节点；
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
- 本机 `OUTPUT` 透明代理；
- 节点全部失效时自动切换 ordinary NAT。

## 3. 固定架构边界

机场订阅安全边界继续保持：

```text
remote subscription
       │
       ▼
 validate / parse
       │
       ▼
   proxies only
```

远端 `proxy-groups`、`rules`、DNS 等策略字段不得进入产品 runtime。

产品职责固定为：

```text
机场：
提供节点

用户：
手动选择需要代理的流量出口

hyz-things：
生成 HYZ-PROXY
决定 DIRECT / PROXY

Mihomo：
执行规则和节点连接
```

## 4. 保持现有 LAN TUN 数据面

本阶段不重构当前数据面：

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

## 5. 产品自有 Proxy Group

只建立一个固定 selector：

```yaml
proxy-groups:
  - name: HYZ-PROXY
    type: select
    proxies:
      - <subscription node 1>
      - <subscription node 2>
      - <subscription node 3>
```

`HYZ-PROXY` 是产品唯一代理出口策略组。

节点列表必须直接来自当前经过验证的订阅 `proxies`：

```text
subscription order
      ↓
HYZ-PROXY order
```

产品不得根据名称猜测节点用途，也不得过滤类似：

```text
官网 tf520.top
剩余流量：154.94 GB
套餐到期时间：...
HYZ-AUTO
```

只要该条目本身是订阅中的合法 proxy 且名称不与产品保留组 `HYZ-PROXY` 冲突，就按原样保留。

## 6. Group 名称 ownership

产品只保留：

```text
HYZ-PROXY
```

如果机场节点名称等于 `HYZ-PROXY`：

```text
candidate invalid
       ↓
拒绝本次订阅更新
       ↓
保留上一份有效配置
```

`HYZ-AUTO` 不再是产品保留名，可作为普通订阅节点名称存在。

## 7. 产品自有 Rules

固定规则：

```yaml
mode: rule

rules:
  - GEOSITE,cn,DIRECT
  - GEOIP,CN,DIRECT,no-resolve
  - MATCH,HYZ-PROXY
```

顺序必须固定：

```text
GEOSITE,cn,DIRECT
        ↓
GEOIP,CN,DIRECT,no-resolve
        ↓
MATCH,HYZ-PROXY
```

不得把 `MATCH` 放在国内规则之前。

`GEOSITE,cn` 负责当前 Mihomo 能获得域名信息时的国内域名直连；`GEOIP,CN` 作为 IPv4 地址层补充。未命中两层国内规则的流量进入 `HYZ-PROXY`。

## 8. GeoSite / GeoIP 数据

正常运行不能依赖设备启动时在线下载 GeoData。

需要固定：

```text
fixed source
fixed version
fixed SHA-256
fixed rootfs path
```

如果 GeoSite / GeoIP 数据缺失、损坏或与当前 Mihomo 不兼容：

```text
candidate validation / readiness fails
          ↓
不得提交新的 managed routing runtime
```

不能静默退化成全量 `MATCH,HYZ-PROXY` 后仍报告正常。

## 9. Runtime 配置生成

订阅刷新流程：

```text
HTTPS fetch
   ↓
validation
   ↓
proxies only
   ↓
generate HYZ-PROXY
   ↓
generate managed GEOSITE/GEOIP rules
   ↓
controlled TUN/listener/controller
   ↓
mihomo -t
   ↓
atomic publish
```

订阅节点新增、删除或排序变化后：

```text
HYZ-PROXY 自动同步
```

用户不需要手工编辑 YAML。

## 10. Managed 字段

以下字段明确归产品所有：

```text
mode
proxy-groups
rules
```

机场或持久 source 中同名字段不能覆盖产品策略。

现有受控 runtime 字段继续保持：

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

## 11. Web 行为

当前 wire model 保持：

```text
ProxyGroup {
    name,
    kind,
    selectable,
    selected,
    options
}
```

节点切换请求继续包含：

```text
group
proxy
```

Proxy 页面只需要展示产品运行时实际存在的 `HYZ-PROXY`：

```text
HYZ-PROXY
[ 日本 / 新加坡 / 香港 / ... ▼ ]
```

页面顶部“当前代理节点”必须明确取：

```text
HYZ-PROXY.selected
```

不得再对不存在的自动策略组做特殊解释。

## 12. 用户选择节点的固定语义

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

国内流量：

```text
LAN
 ↓
TUN
 ↓
GEOSITE/GEOIP CN
 ↓
DIRECT
```

foreign 流量：

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

## 13. Device Policy

保持当前 device policy：

```text
Direct
Proxy
```

`Direct` 继续在 TUN mangle chain 中通过 MAC `RETURN`，完全不进入 Mihomo。

`Proxy` 的含义仍然是：

> 使用 Mihomo TUN 分流策略。

不是强制所有流量使用机场。

## 14. 本机系统代理

本机系统代理继续使用：

```text
127.0.0.1:7890
```

并与 LAN TUN 共享 Mihomo core 和相同 rules：

```text
GEOSITE,cn → DIRECT
GEOIP,CN → DIRECT
MATCH → HYZ-PROXY
```

## 15. Subscription Refresh 与回滚

更新失败必须保留上一份正常工作的 source/runtime。

不能因为新机场订阅出现：

```text
节点重名
HYZ-PROXY 名称冲突
无有效节点
Mihomo validation failure
```

而破坏当前正在工作的代理。

## 16. 节点延迟刷新

延迟刷新是产品 UI/Panel 能力，不再依赖产品自动选择组。

`HYZ-PROXY` 包含当前所有订阅节点，因此产品可以对该组执行有界 group delay 请求并将结果缓存到展示模型。

当前 follow-up 固定：

```text
Mihomo delay timeout: 10 s
Controller transport timeout for delay requests: 30 s
normal Controller timeout: 8 s
```

这些 timeout 只是管理面探测边界，不改变实际代理连接的 routing policy。

## 17. Host 测试

至少覆盖：

```text
subscription strips remote rules
subscription strips remote proxy-groups
HYZ-PROXY includes all current subscription nodes in order
subscription node names are not heuristically filtered
HYZ-AUTO is allowed as an ordinary subscription node name
HYZ-PROXY name collision is rejected
only one managed proxy group is generated

GEOSITE,cn appears before GEOIP,CN
GEOIP,CN appears before MATCH
MATCH always targets HYZ-PROXY

HYZ-PROXY Selector is selectable
selecting a node sends group + proxy
subscription refresh preserves last good config on invalid candidate
LAN TUN desired state is unchanged by node selection
local-system desired state is unchanged by node selection
```

普通 host 测试不得执行真实：

```text
iptables
ip
TUN
WAN access
real provider
```

## 18. Web E2E

Proxy 页面至少验证：

```text
只展示一个 HYZ-PROXY selector
HYZ-PROXY 可手动选择
节点列表包含当前订阅节点
用户选择实际节点后页面显示该节点
节点切换请求仍包含 group + proxy
LAN TUN toggle 不受节点修改影响
本机系统代理 toggle 不受节点修改影响
```

继续优先使用 role、accessible name 和稳定产品文案作为 E2E selector，不依赖 Tailwind class 或 DOM hierarchy。

## 19. 目标板验收

至少包括：

1. TUN disabled 时 ordinary NAT 正常；
2. TUN enabled 后手机无需配置 HTTP proxy；
3. 一个确认属于国内域名集合的目标命中 `GEOSITE,cn,DIRECT`；
4. 一个确认属于 CN IPv4 的目标命中 `GEOIP,CN,DIRECT`；
5. 一个确认属于 foreign 的目标命中 `HYZ-PROXY`；
6. 用户选择节点 A，新的 foreign connection 使用节点 A；
7. 切换到节点 B，新的 foreign connection 使用节点 B；
8. 切换节点前后，CN connection 始终 `DIRECT`；
9. subscription refresh 后新增节点进入 `HYZ-PROXY`；
10. subscription refresh 后删除节点退出 `HYZ-PROXY`；
11. 机场自身 `rules` 不影响最终分流；
12. Direct device 继续绕过 TUN；
13. Mihomo core `SIGKILL` 后 ordinary NAT fail-open 不回归；
14. router restart 后 TUN ownership/order 不回归；
15. reboot 后 desired TUN 状态正常恢复；
16. 本机无 `OUTPUT` interception；
17. Tailscale 行为不变。

不得记录订阅 URL/token、proxy password、节点 credential 或 controller secret。

## 20. Host 验证

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

修改 Web 后对应执行 things native tests、frontend 和 Playwright E2E。

仓库 guardrail 要求：未获得明确构建授权时，不在本地启动 Rust/frontend/SDK/Buildroot 构建；所有未运行项目必须明确标记。

## 21. 完成标准

只有满足以下条件，本文才能标记为完成：

- subscription 仍然只接受 `proxies`；
- 机场 `rules` 不进入 runtime；
- 机场 `proxy-groups` 不进入 runtime；
- 只生成一个 `HYZ-PROXY`；
- `HYZ-PROXY` 包含当前订阅的全部节点并保持顺序；
- 不做节点名称启发式过滤；
- 不生成 `HYZ-AUTO`；
- `GEOSITE,cn,DIRECT` 生效；
- `GEOIP,CN,DIRECT,no-resolve` 生效；
- 最终 `MATCH` 固定进入 `HYZ-PROXY`；
- 用户节点选择只影响 proxy traffic；
- CN traffic 不因用户节点选择而进入机场；
- 分组测速使用独立且有界的 10 秒探测窗口和 30 秒 Controller 传输超时；
- Web 以 `HYZ-PROXY` 作为唯一产品代理组；
- Direct device policy 不回归；
- LAN TUN 与本机系统代理独立开关不回归；
- existing ordinary NAT fail-open 不回归；
- host tests 和 Web E2E 通过；
- 真实板端 `GEOSITE CN DIRECT / GEOIP CN DIRECT / foreign PROXY` 已验证；
- 未执行项目明确标记。
