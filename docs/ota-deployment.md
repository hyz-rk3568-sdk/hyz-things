# RK3568 OTA 部署流程

本文定义 `output/upgrade.fw` 到开发板的日常 recovery-free OTA 传输、安装和验收流程。正常 OTA 不更新 recovery，也不写入 userdata 分区镜像。

## 传输优先级

1. **USB 有线 ADB 优先。** 链路稳定，允许使用 `adb push` 传输固件。
2. **网络 ADB 次选。** 不使用 `adb push` 传输数百 MiB 的固件；临时开启仅绑定宿主机局域网地址的 HTTP server，由板端 `hyz-router ota download` 流式下载并校验。

网络 ADB 适合没有 USB 连接的现场调试，但 STA 切换、OTA 重启或 DHCP 地址变化都会使现有 `IP:5555` transport 断开。断开本身不能作为失败判据。

## 安装前检查

从工作区根目录确认产物和摘要：

```sh
SHA256=$(cut -d' ' -f1 output/upgrade.fw.sha256)
sha256sum output/upgrade.fw
stat -c '%n %s bytes' output/upgrade.fw
```

安装前必须完成以下审计：

- `output/upgrade.fw` 的主机 SHA-256 与 `output/upgrade.fw.sha256` 一致；
- 常规包成员只有 bootloader、U-Boot、misc、boot、rootfs 和 oem，不含 recovery、userdata；
- 从打包 rootfs 提取的 `/usr/bin/hyz-router` 与本次 AArch64 构建 ELF 逐字节一致；
- ADB 目标的 `product/model/device` 为预期的 `hyz_things/HYZ_RK3568/rk3568`；
- 板端不存在另一个 pending OTA command。

## 方式一：USB 有线 ADB

确认并固定目标 serial，避免多设备时误操作：

```sh
adb devices -l
SERIAL=USB_SERIAL
adb -s "$SERIAL" get-state
```

传输后必须在板端重新计算摘要：

```sh
adb -s "$SERIAL" push output/upgrade.fw /userdata/upgrade.fw
DEVICE_SHA=$(adb -s "$SERIAL" shell sha256sum /userdata/upgrade.fw \
  | tr -d '\r' | awk '{print $1}')
test "$DEVICE_SHA" = "$SHA256"
```

先写入 OTA staging/BCB，再显式重启：

```sh
adb -s "$SERIAL" shell \
  "hyz-router ota install /userdata/upgrade.fw '$SHA256'"
adb -s "$SERIAL" reboot
```

不要把 `adb wait-for-device` 当作完整启动成功；ADB 会早于路由服务 readiness 上线。

## 方式二：网络 ADB + 临时 HTTP server

连接并固定网络 serial：

```sh
DEVICE_IP=BOARD_STA_IP
SERIAL="$DEVICE_IP:5555"
adb connect "$SERIAL"
adb -s "$SERIAL" get-state
```

根据到板子的实际路由选择宿主机局域网地址，不要硬编码历史地址：

```sh
HOST_IP=$(ip route get "$DEVICE_IP" \
  | awk '{for (i=1; i<=NF; i++) if ($i=="src") {print $(i+1); exit}}')
PORT=18000
test -n "$HOST_IP"
```

从工作区根目录启动临时 server。只绑定选定的局域网地址，不使用 `0.0.0.0`：

```sh
python3 -m http.server "$PORT" --bind "$HOST_IP" --directory output
```

在另一个终端先用小文件验证板到宿主机的 HTTP 路径：

```sh
adb -s "$SERIAL" shell \
  "wget -qO- http://$HOST_IP:$PORT/upgrade.fw.sha256"
```

然后让板端 OTA 客户端流式下载。该命令校验 SHA-256 和 RKFW header，成功后原子提交到固定 staging 路径：

```sh
adb -s "$SERIAL" shell \
  "hyz-router ota download http://$HOST_IP:$PORT/upgrade.fw '$SHA256'"
```

看到以下成功结果后立即停止临时 HTTP server：

```text
downloaded and verified /userdata/hyz-router/ota/upgrade.fw
```

安装已验证的 staging 文件并重启：

```sh
adb -s "$SERIAL" shell \
  "hyz-router ota install /userdata/hyz-router/ota/upgrade.fw '$SHA256'"
adb -s "$SERIAL" reboot
```

网络 ADB 会在 recovery 安装和重启期间离线。等待原地址恢复时可以重新执行：

```sh
adb connect "$SERIAL"
```

如果 STA 配置或 DHCP lease 已变化，应从上游 DHCP 状态确认新地址，不要无限重试旧地址。

### 网络传输失败处理

- `adb push` 在网络 ADB 上可能长时间停在部分文件大小；确认不再增长后终止该次 push，改用 HTTP 流式下载。
- 中断的 push 可能留下 `/userdata/upgrade.fw` 部分文件。只有确认它是本次生成的传输残留、且 BCB/OTA staging 不引用它后，才进行清理。
- HTTP 下载失败时，`hyz-router` 不会把未验证的临时文件提交为正式 staging；检查错误后可重新执行 download。
- 不要因为网络 ADB 断开而重复写 BCB。重连后先检查板端 ELF、uptime 和 router status，判断 OTA 是否已经完成。

## 安装后验收

先验证安装 ELF 与打包 ELF 的 SHA-256 完全一致：

```sh
adb -s "$SERIAL" shell sha256sum /usr/bin/hyz-router
```

再等待完整 readiness，而不是只等待 ADB：

```sh
adb -s "$SERIAL" shell hyz-router status
```

日常路由 + TUN 配置至少应满足：

- 顶层 `state` 为 `ok`；
- router、proxy、system 均为 `available`；
- STA、默认路由、AP、`br-lan` attachment、IPv4 forwarding 和 MASQUERADE 已确认；
- 期望 LAN TUN 开启时，proxy 的 `lan_tun.desired=true`、`lan_tun.effective=ready`，且 Mihomo process/runtime config/mixed port 均为 ready；
- userdata 中的网络、管理员和订阅配置仍存在，但验收脚本不得读取或输出其秘密内容。

## 安全边界

- 临时 HTTP server 只用于受控开发局域网，下载完成后立即停止。
- SHA-256 只能防止传输损坏，不能提供发布者认证；它不能替代签名、anti-rollback 或 A/B 回滚。
- 不在命令、日志或文档中记录 Wi-Fi 密码、订阅 URL/token、代理节点凭据或管理员密码。
- recovery 更新必须使用独立的 `upgrade-recovery.fw` 和显式 recovery 安装流程，不能把常规 OTA 替换为 recovery OTA。
