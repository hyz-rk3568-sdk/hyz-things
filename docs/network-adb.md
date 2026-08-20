# 网络 ADB 连接指南

目标板 `hyz_things`（RK3568）上的 adbd 来自 Buildroot 包 `android-tools`
（4.2.2+git20130218 的 Debian 移植 + Rockchip 补丁）。本文记录本机
（WSL）通过局域网使用网络 ADB 的正确方法，以及曾导致"网络 ADB 不可用"
误判的协议细节。

## 结论

- 板端网络 ADB **可用**，且 **fresh boot 后无需手动启用**（已实测）。
- 板端 adbd 是"客户端先发 CNXN"的老协议；`adb connect` 后设备未列出或
  连接表现异常，不一定是网络 ADB 不可用，先按下面步骤正确连接再验证。
- 本 WSL 上默认 `adb` 客户端连的是 Windows 的 adb server
  （localhost:5037 转发）。必须使用 WSL 独立 server（端口 5038）。控制 router 的网络 ADB 可以选择物理 LAN 或可达的 Tailscale 路径；使用前确认 `ip route get <BOARD_IP>` 走预期接口，并确认目标地址的 TCP 5555 已开放。

## WSL 连接步骤

```sh
DEVICE_IP=BOARD_NETWORK_IP
adb -P 5038 start-server            # WSL 独立 server，避开 Windows 的 5037
adb -P 5038 connect "$DEVICE_IP:5555"
adb -P 5038 -s "$DEVICE_IP:5555" shell id
```

`BOARD_NETWORK_IP` 可以是板端 Ethernet/Wi-Fi 上游地址，也可以是明确开放 TCP 5555 的 Tailscale 地址。

- 板端 adbd 从启动即监听 TCP 5555：Buildroot 配置
  `BR2_PACKAGE_ANDROID_TOOLS_TCP_PORT=5555` 生成
  `/etc/profile.d/adbd.sh`（`export ADB_TCP_PORT=5555`），配合 Rockchip
  补丁 0018 让 adbd 开机就监听该端口。
- `adb tcpip 5555` 在这版 adbd 上只做 ACK（property 写入被注释掉），但
  运行中的 adbd 随后即服务 TCP；它只用于没有开机 TCP 监听的环境。
- 不要用不带 `-P 5038` 的 `adb` 连板子：会命中 Windows adb server 并
  10060 超时。
- 老版 adbd 连接建立前对端不可见；判断连接成功用 `get-state` 和
  `shell id`，不要用"是否立即列出"。

## 部署工具用法

`deploy-app.sh`、`make deploy-things` 和 `make deploy-camera` 等工具以 `ADB`/`ADB_SERIAL`
变量驱动 adb。router 开发热替换不使用 `deploy-app.sh`：USB ADB 可以执行受控 `stop → 替换 → start`；网络 ADB 必须在旧 router 仍运行时完成 staging、SHA-256/协议检查和原子替换，然后 `adb reboot`，不能先 stop。router 正式发布仍必须走 OTA。

```sh
DEVICE_IP=BOARD_NETWORK_IP
ADB="adb -P 5038" ADB_SERIAL="$DEVICE_IP:5555" \
  bash apps/rust/things/tools/deploy-app.sh deploy things <ELF>
```

router 开发热替换不使用 `deploy-app.sh`；网络 ADB 路径必须遵循 [`router-app-debug.md`](router-app-debug.md) 的“原子替换、reboot”流程。

- USB 场景：`ADB="adb"`，`ADB_SERIAL=<USB serial>`。
- 网络 ADB 场景：`ADB="adb -P 5038"`，`ADB_SERIAL="$DEVICE_IP:5555"`；`$DEVICE_IP` 可以走物理 LAN 或 Tailscale，但必须确认路径和 TCP 5555 可达。

## 连接/断开语义

- STA 切换、OTA 重启或 DHCP 地址变化会使现有 `IP:5555` transport 断开；
  断开本身不是失败判据，按 [ota-deployment.md](ota-deployment.md) 的流程
  等待恢复或从 DHCP 状态确认新地址。

## Tailscale 网络 ADB

Tailscale 地址可以作为网络 ADB 地址使用，但必须由设备防火墙明确允许 TCP 5555。网络 ADB router 热替换仍必须使用“原子替换、reboot”流程；不能通过 Tailscale ADB 先执行 router stop，因为 shutdown 可能同时清理 Tailscale runtime，导致控制通道断开。

## 传输注意

- 网络 ADB 不做大文件 `adb push`（可能长时间停在部分文件大小）；OTA
  固件按 [ota-deployment.md](ota-deployment.md) 方式二用临时 HTTP server
  流式下载。
