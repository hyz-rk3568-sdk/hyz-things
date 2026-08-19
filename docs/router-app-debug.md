# hyz-router 修改验证与发布

`hyz-router` 拥有 LAN、WAN DHCP、转发/NAT、Mihomo、Tailscale、OTA 和关机生命周期。它直接管理设备网络与防火墙，因此正式发布不使用 USB ADB 热替换或开发启动覆盖。

## 开发板验证方式

开发阶段在已授权的 RK3568 设备上验证已经构建的 `hyz-router` binary **不需要制作 OTA**。验证流程是：

1. 停止旧 daemon，备份设备上的旧 binary；
2. 将候选 binary 上传到临时路径，校验主机和设备 SHA-256 后再替换；
3. 执行 `/etc/init.d/S81hyz-router start`，等待 `/run/hyz-router/ready`；
4. 执行双上行状态矩阵和需要覆盖的回退、恢复、ownership 测试；
5. 执行 `/etc/init.d/S81hyz-router stop`，确认快速返回并复核进程、地址、路由、防火墙、bridge、lock、socket、ready 和 ownership 没有残留。

`start` + `stop` 只覆盖 daemon 生命周期；它们不能替代网络状态矩阵，也不能证明 OTA 安装、启动和回滚流程正确。开发板上的直接 binary 验证也不代表正式发布物已经完成 OTA 验收。

## 发布方式

正式发布和 OTA 安装验证必须通过 OTA：

```sh
make upgrade
```

需要显式包含 recovery 时使用：

```sh
make upgrade-recovery
```

OTA 构建、传输、校验、安装和回滚流程见 [`docs/ota-deployment.md`](ota-deployment.md)。设备端必须在安装前后校验固件身份，并通过新的 boot、router readiness、Mihomo、Tailscale 和管理 HTTP readiness 完成验收。

## 禁止的 router 部署方式

以下方式不能作为正式发布或绕过 ownership/readiness 事务的手段：

- 通过 `S80hyz-router-dev` bind mount 覆盖 `/usr/bin/hyz-router`；
- 通过 ADB 直接替换或启动 `hyz-router` 作为发布方式；开发板临时验证必须遵循上面的备份、SHA-256、`start`、矩阵、`stop` 和残留复核流程；
- 通过 `deploy-app.sh` 热推送 router ELF；
- 通过临时 init 脚本绕过 OTA 或 router 的 ownership/readiness 事务。

`apps/rust/things/tools/deploy-app.sh` 只允许热部署 `hyz-things` 和 `hyz-camera`。这两个应用的热部署不得停止或重启 router；router 版本和 wire contract 变化必须通过 OTA 一起发布。

## 开发验证边界

主机侧继续使用 crate 级格式化、clippy、host 测试和静态脚本检查。真实设备上的开发板验证可以使用受控的 binary staging 加 `start`/`stop`，但仍必须通过状态矩阵和 cleanup 复核；普通 host 测试不执行目标设备上的网络、iptables、`/run`、`/userdata` 或进程状态修改。
