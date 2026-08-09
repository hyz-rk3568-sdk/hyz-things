# Mihomo 与历史 MetaCubeXD 固件验证记录

## 状态

**Mihomo 的显式代理和仅 `br-lan` 入站 TUN 已通过本文记录的 recovery-free OTA、重启持久性、手机 TCP/UDP/视频、router restart 和核心退出普通 NAT 回退验证。MetaCubeXD 只在该历史固件中验证过静态文件安装，从未启用 Controller；当前源码已删除其 Buildroot 包，产品 Web UI 统一由 `hyz-router` 内嵌 Yew 提供。**

验证日期：2026-08-09。

历史显式代理阶段建立了 `PX-01` 和 `PX-02` 的固件基础；当前新增 TUN 源码仍不修改 Ethernet 或 PPPoE，也不把任何真实订阅、节点或密码放入 rootfs。TUN 的规则、模式和失败边界详见 [`soft-router-tun.md`](soft-router-tun.md)。

## 历史固定输入

| 组件 | 固定版本和资产 | SHA-256 | 许可证 |
| --- | --- | --- | --- |
| Mihomo | `v1.19.29`，`mihomo-linux-arm64-v1.19.29.gz` | `9a868b5e4e0ad91d9d71e1b41b0cfce78aaba44360c30df74a723f8e3926a86c` | GPL-3.0，同 tag `LICENSE` 哈希已固定 |
| MetaCubeXD | `v1.271.0`，`compressed-dist.tgz` | `6b977903db4038d2737592342ab550231878fd410fa3d436fad28765f4d0afc7` | MIT，同 tag `LICENSE` 哈希已固定 |

Mihomo 采用上游官方预构建 Linux ARM64 二进制。下载后确认它是 stripped、静态链接的 AArch64 ELF；本阶段没有从 Go 源码重建。历史固件中的 MetaCubeXD 只安装静态 Web 发布物，未引入 Node.js、Tauri、WebView 或板端前端构建工具。

当时两个包使用版本化 Buildroot 下载目录，避免 MetaCubeXD 每个 Release 都复用 `compressed-dist.tgz` 文件名时命中旧缓存。以下包清单、哈希和镜像尺寸仅用于复现已完成的历史验证，不代表当前产品配置。

## 历史固件集成

当时的 Buildroot 产品配置启用：

```text
BR2_PACKAGE_MIHOMO=y
BR2_PACKAGE_METACUBEXD=y
```

生成 rootfs 包含：

```text
/usr/bin/mihomo
/usr/sbin/hyz-mihomo
/etc/init.d/S82hyz-mihomo
/etc/hyz-router/mihomo-config.yaml.example
/usr/share/metacubexd/
```

ext4 镜像内的二进制和脚本属于 `root:root`：Mihomo、生命周期脚本和 SysV 脚本模式为 `0755`，无秘密模板和 Dashboard 文件模式为 `0644`。MetaCubeXD 共安装 158 个静态文件，约 8.04 MB；未把 Buildroot `.stamp*` 或额外 `LICENSE` 文件错误复制到 Web 根目录。

### 生命周期和配置边界

`hyz-mihomo` 支持 `start`、`stop`、`restart`、`status` 和 `check`：

- 真实配置只从 `/userdata/hyz-router/mihomo/config.yaml` 读取；
- 配置不存在时返回状态 `3`，`S82hyz-mihomo` 输出 `SKIP`，不创建默认开放代理；
- 启动前将配置目录限制为 `0700`、配置文件限制为 `0600`；
- 拒绝仍含 `CHANGE_ME_` 占位符的配置；
- 第一阶段要求 `allow-lan: true` 且 `bind-address: 192.168.8.1`；
- 调用 Mihomo 自身 `-t` 校验通过后才启动；
- PID、配置校验日志和运行日志位于 `/run/hyz-mihomo/`，不持续写入 userdata；
- stop 前校验 `/proc/<pid>/exe`，避免误杀无关进程。

当时的默认模板只提供带认证的 mixed-port 和 `MATCH,DIRECT` 安全骨架；External Controller 与 MetaCubeXD 入口始终保持注释关闭。当前模板已删除这些入口，统一 Web UI 不依赖 Mihomo Controller。

## 编译结果

执行：

```text
make upgrade
make check
```

完整 recovery-free OTA 编译成功。`make check` 同时通过 Rust OTA 6 个单元测试、产品/Recovery Kconfig 解析、代理包 `check-package`（84 行、0 告警）、脚本语法、固定版本和 OTA 分区清单检查。

| 产物 | SHA-256 | 大小 |
| --- | --- | ---: |
| `output/upgrade.fw` | `cd16267b9581122dc9f727c614c189f7eae34871179142e3bc68da92d2aaba08` | 416,957,002 bytes |
| 打包 `boot.img` | `5b039ce79aac8213e59b131bad5021c15daef5ef6f454d9a2d5ec413952046a9` | — |
| 打包 `rootfs.img` | `5e590c789ae313d61e4fe304e3c2b36d41a1e775e39ceaa182fe2051c1f59357` | — |
| rootfs `/usr/bin/mihomo` | `973f010a28980f553446776ceef88dafca9356828a4251399f689dbe9677d2db` | 44,195,440 bytes |
| rootfs `/usr/sbin/hyz-mihomo` | `a012f415a74bb36557ef42df63bc91b98ae85f92616ee99a8e527431527bb80c` | 3,270 bytes |
| MetaCubeXD `index.html` | `27767d9a68d035c7226707d91cf651665622fe70479e99d0b76262c919b3faa0` | 5,106 bytes |

`upgrade.fw.sha256` 与实际 OTA 文件一致。打包清单不包含 recovery 或 userdata，因此本次仍是 recovery-free OTA。

生成日志：

```text
output/mihomo-explicit-proxy-build.log
output/mihomo-explicit-proxy-check.log
```

以上日志和固件均为生成物，不提交 Git。

## 板端 OTA 结果

板端先对 `/userdata/upgrade-mihomo-v1.19.29.fw` 重新计算 SHA-256 和字节数，均与主机产物一致；
`hyz-ota install` 验证并写入 recovery-free 安装控制信息后，设备经过 recovery 返回新的产品 rootfs。

板端验证结果：

- 产品 rootfs 仍来自 `/dev/mmcblk0p6`，主机名仍为 `hyz-things`；
- `/usr/bin/mihomo -v` 报告 Mihomo Meta `v1.19.29`、Linux ARM64；
- 板端 Mihomo、`hyz-mihomo` 和 MetaCubeXD `index.html` SHA-256 与构建 rootfs 完全一致；
- MetaCubeXD 仍包含 158 个静态文件；
- `/userdata/hyz-router/mihomo/config.yaml` 不存在，`hyz-mihomo status` 正确返回
  `mihomo=disabled config=missing`；
- 串行扫描 `/proc/*/exe` 未发现 Mihomo 常驻进程，TCP/UDP `7890` 和 Controller `9090` 均未监听；
- recovery SHA-256 仍为 `9778353401cf31b6bdca54fd1e5dd59a7a0983d9b32488eb89e3f974f036e363`；
- 原有 STA/AP userdata 配置在 OTA 前后哈希一致，未读取或记录配置内容；
- `wlan0` STA 为 `COMPLETED`，默认路由 metric `600`，`p2p0` AP 为 `ENABLED`；
- IPv4 forwarding、`HYZ_ROUTER_FWD`、`HYZ_ROUTER_NAT` 和 MASQUERADE 均存在；
- 板端经 `wlan0` 访问互联网 3 次 ping 无丢包。

验证时没有下游 AP 客户端保持关联，因此本轮只证明基础路由控制面和板端上游联网未回归，不能替代
下游客户端 NAT、DNS、直播或显式代理流量验收。

## 板端认证 DIRECT 自测

在设备内生成随机 mixed-port 密码，密码未回显到主机、普通日志或文档。测试配置目录模式为 `0700`，
配置文件模式为 `0600`，只包含：

- mixed-port `7890`；
- `allow-lan: true`；
- `bind-address: 192.168.8.1`；
- 用户名和设备内随机密码认证；
- `MATCH,DIRECT`；
- 不启用 External Controller、Dashboard、TUN、DNS 接管或真实代理节点。

板端结果：

1. `hyz-mihomo check` 调用 Mihomo `-t` 通过；
2. 连续两次 `start` 保持同一 PID，未生成重复进程；
3. 只监听 `192.168.8.1:7890`，未监听 `0.0.0.0:7890` 或 `9090`；
4. 不带认证的原始 HTTP 代理请求返回 `HTTP/1.1 407 Proxy Authentication Required`；
5. 使用只在设备内构造的 Basic 凭据，经代理访问 `http://example.com/` 返回 `HTTP/1.1 200 OK`；
6. 运行日志不包含随机认证密码；
7. stop 后 PID 文件、Mihomo 进程和 `7890`/`9090` 监听均已清理；
8. 测试后 STA、AP、metric `600`、forwarding、MASQUERADE 和板端上游联网保持正常。

该请求从 RK3568 本机发起，只验证核心、认证、DNS/TCP DIRECT 出站及生命周期，不能替代从 `p2p0`
下游客户端发起的显式代理验收。测试完成后最初将配置以 root-only `direct-tested.yaml` 归档，不保留活动的 `config.yaml`；再次执行 SysV
start 得到 `SKIP`，证明配置缺失时不会自动开放端口。后续持久开关 OTA 已补充独立 `enable`/`disable`
命令和 root-only `disabled` marker，相关结果见下一节。

## 持久开关与真实 provider 自测

`hyz-mihomo` 新增持久 `enable` 和 `disable`：

- `disable` 先创建模式 `0600` 的 `/userdata/hyz-router/mihomo/disabled`，再停止进程；
- marker 存在时，普通 `start` 和开机 SysV 入口均返回 `SKIP`；
- `enable` 要求活动配置存在，移除 marker 后执行完整配置校验和启动；
- enable 失败时恢复 marker、停止残留进程并保持 fail-closed；
- `status` 区分 marker 禁用、配置缺失、已停止、运行和异常禁用但仍运行。

源码脚本在板端临时验证通过后，重新生成并安装 recovery-free OTA：

| 项目 | SHA-256 |
| --- | --- |
| 持久开关 `upgrade.fw` | `8c7d5121bb700a0782fef6fff5d43867716f38eade368e9b7bee09fa48d6eaf0` |
| 打包 `rootfs.img` | `ea680ad95085870e6bcda9990e0ae8aa82a776940105c498242b17387b505b12` |
| rootfs `/usr/sbin/hyz-mihomo` | `54c024a6ec1e5651579ae9f0d077564bc60a2eeb37b6b6fa28544f868eeab511` |

OTA 后 recovery 哈希仍为既有基线，订阅、provider、候选配置和原 STA/AP userdata 配置均保留。

订阅处理和测试全程不把 URL、token、节点名、节点凭据或 mixed-port 密码写入 Git、产品文档、构建日志
或普通运行日志：

1. URL 只保存在 root-only `subscription.url`；`wget` 使用 input-file，URL 不进入 wget 命令行；
2. 严格 HTTPS 获取通过，并在设备内解析为受支持的 provider 数据；
3. 使用 `clash.meta` User-Agent 得到可由 Mihomo 校验的 Clash YAML；
4. 不直接采用服务端完整配置；其校验失败只命中 GeoIP/GeoSite 下载类别，未命中 YAML 解析或协议不支持；
5. 只在设备内提取 `proxies` 到 root-only 本地 provider，并使用产品自有 LAN 绑定、认证、规则和生命周期；
6. 受控候选配置不包含订阅 URL，Mihomo `-t` 校验通过；
7. 配置在 disabled marker 保护下原子启用，当前仅监听 `192.168.8.1:7890`，Controller `9090` 仍关闭。

真实 provider 的 RK3568 本机自测通过：未认证 HTTP 返回 `407`；认证 HTTP 返回 `200`；认证 HTTPS
下载通过；Mihomo 日志新增 2 条 `using PROXY` 规则命中；同时 `wget --no-proxy` 普通直连 HTTPS 通过，
日志未出现 mixed-port 密码。STA、AP、metric `600`、forwarding 和 MASQUERADE 保持正常。

该测试证明至少一个真实 provider 路径能代理本机 HTTP/HTTPS，但当时 `p2p0` 关联客户端数为 0，
仍不能替代下游显式代理验收。真实下游测试结果见下一节；Dashboard 和透明代理保持关闭。

## 下游显式代理实测

一台手机关联 `p2p0` 并获得 DHCP 租约后，使用一次性 mixed-port 凭据手工配置
`192.168.8.1:7890`。用户确认可以联网，但视频无法正常播放。

手机活跃期间连续采样 30 秒：

| 指标 | 结果 |
| --- | ---: |
| `p2p0` 下发到手机 | 约 `1.055 Mbps` |
| 手机上传到 `p2p0` | 约 `0.033 Mbps` |
| `wlan0` 上游下载 | 约 `1.092 Mbps` |
| 新增 `using PROXY` 命中 | 1 |
| 新增 timeout | 0 |
| 新增 connection error | 0 |
| 上游网关 ping | 0% 丢包，平均 `352 ms`，最高 `1.20 s` |

当时 `wlan0` RSSI 为 `-79 dBm`，STA 和 AP 均位于 2437 MHz/信道 6。驱动仍无法提供有效的下游
station signal，报告值为 `0 dBm`，不能用来判断手机侧 RSSI。

板端使用相同真实 provider 下载同一 5 MB HTTPS 测试对象达到约 `4.689 Mbps`，耗时约 8.53 秒，
说明所选节点至少具备高于手机实测流量的吞吐，且代理日志无连接错误。相同对象的板端直连路径本轮异常地
只有约 `0.230 Mbps`，与此前约 `3.78 Mbps` 的 STA 直连基线不一致，因此该单次 direct 结果不能用于
计算稳定的代理损耗比例。

综合判断：手机流量确实通过真实 provider，视频失败不是认证、规则未命中或节点完全不可用；主要限制仍是
`-79 dBm` 弱上游信号和同一 RTL8852BS 在同信道完成 STA 接收与 AP 转发造成的 airtime 排队。当前结果
只能判定“下游显式代理基础联网通过、视频性能未通过”，不能完成 `PX-03` 的吞吐和稳定性验收。

测试结束后已持久 disable Mihomo、撤销聊天中显示的一次性 mixed-port 密码、恢复设备内隐藏凭据，并确认
进程、PID 和 `7890`/`9090` 监听均已清理。STA+AP+NAT 继续正常。

## 尚未通过

- 尚未验证下游 UDP、节点切换、2 小时稳定性和可接受的视频吞吐；
- 仅 `br-lan` 入站的 TUN、策略路由、排除规则和核心退出清理已通过最终 OTA 与板端验证；DNS 接管、live-hang 和节点全部失效自动回退仍未实现；
- 尚未执行 2 小时显式代理或 8 小时代理稳定性测试；
- 正式发布前仍需评估 GPL 预构建二进制的对应源码归档和分发合规流程。

因此本结果证明 Mihomo 能够以固定输入装入固件、通过 recovery-free OTA 安装，并完成持久开关、本机真实
provider HTTP/HTTPS 和下游基础显式代理联网；下游视频性能未通过，它不代表全部代理或统一 Web UI 板端验收已经完成。

真实订阅 URL、代理节点、用户名、密码、API secret、证书和私钥不得写入本文、Git、固件模板或普通日志。
