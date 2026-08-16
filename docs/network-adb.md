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
  （localhost:5037 转发），Windows 路由不到 192.168.8.1（10060 超时）。
  必须使用 WSL 独立 server（端口 5038）。

## WSL 连接步骤

```sh
adb -P 5038 start-server            # WSL 独立 server，避开 Windows 的 5037
adb -P 5038 connect 192.168.8.1:5555
adb -P 5038 -s 192.168.8.1:5555 shell id   # 验证（serial 是 IP:5555）
```

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

`deploy-app.sh`、`make router-deploy-dev` 等工具以 `ADB`/`ADB_SERIAL`
变量驱动 adb：

```sh
ADB="adb -P 5038" ADB_SERIAL="192.168.8.1:5555" \
  bash apps/rust/things/tools/deploy-app.sh deploy things <ELF>
```

- USB 场景：`ADB="adb"`，`ADB_SERIAL=<USB serial>`。
- 网络场景：`ADB="adb -P 5038"`，`ADB_SERIAL="192.168.8.1:5555"`。

## 连接/断开语义

- STA 切换、OTA 重启或 DHCP 地址变化会使现有 `IP:5555` transport 断开；
  断开本身不是失败判据，按 [ota-deployment.md](ota-deployment.md) 的流程
  等待恢复或从 DHCP 状态确认新地址。

## Tailscale 路径（未开放）

- `100.89.103.59:5555` 不通：板端防火墙只放行 8080 管理端口。
- 开放 5555 等于给网内设备 root shell，属于安全决策；当前保持关闭，
  如需开放需明确确认。

## 传输注意

- 网络 ADB 不做大文件 `adb push`（可能长时间停在部分文件大小）；OTA
  固件按 [ota-deployment.md](ota-deployment.md) 方式二用临时 HTTP server
  流式下载。
