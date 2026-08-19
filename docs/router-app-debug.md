# hyz-router 修改与发布

`hyz-router` 拥有 LAN、WAN DHCP、转发/NAT、Mihomo、Tailscale、OTA 和关机生命周期。它直接管理设备网络与防火墙，因此不支持通过 USB ADB 热替换或开发启动覆盖。

## 发布方式

Router 修改必须通过 OTA 发布：

```sh
make upgrade
```

需要显式包含 recovery 时使用：

```sh
make upgrade-recovery
```

OTA 构建、传输、校验、安装和回滚流程见 [`docs/ota-deployment.md`](ota-deployment.md)。设备端必须在安装前后校验固件身份，并通过新的 boot、router readiness、Mihomo、Tailscale 和管理 HTTP readiness 完成验收。

## 禁止的 router 部署方式

以下方式已移除，不应恢复：

- 通过 `S80hyz-router-dev` bind mount 覆盖 `/usr/bin/hyz-router`；
- 通过 ADB 直接停止、替换或启动 `hyz-router`；
- 通过 `deploy-app.sh` 热推送 router ELF；
- 通过临时 init 脚本绕过 OTA 或 router 的 ownership/readiness 事务。

`apps/rust/things/tools/deploy-app.sh` 只允许热部署 `hyz-things` 和 `hyz-camera`。这两个应用的热部署不得停止或重启 router；router 版本和 wire contract 变化必须通过 OTA 一起发布。

## 开发验证边界

主机侧继续使用 crate 级格式化、clippy、host 测试和静态脚本检查。真实设备上的 router 修改只能通过 OTA 验证，不在普通 host 测试中执行网络、iptables、`/run`、`/userdata` 或进程状态修改。
