# hyz-router 修改验证与发布

`hyz-router` 拥有 LAN、WAN DHCP、转发/NAT、Mihomo、Tailscale、OTA 和关机生命周期。它直接管理设备网络与防火墙，因此开发板调试可以使用受控的 ELF 热替换，正式发布仍必须使用 OTA。

## 开发板验证方式

开发阶段在已授权的 RK3568 设备上验证已经构建的 `hyz-router` binary 不需要制作 OTA。router 热替换分为 USB ADB 和网络 ADB 两条路径。

### USB ADB：stop、替换、start

USB ADB 不依赖板端管理网络，可以执行完整的 stop/start 流程：

1. 执行 `make router-app` 构建候选 ELF；
2. 候选 ELF 上传到设备临时路径，核对主机和设备 SHA-256，并检查与已运行 things 的 wire protocol 兼容性；
3. 备份设备上的旧 `/usr/bin/hyz-router`，同时记录备份 SHA-256；
4. 执行 `/etc/init.d/S81hyz-router stop`，等待进程、socket、ready、bridge、地址、路由、防火墙和 ownership runtime 清理；
5. 将候选 ELF 安装到临时目标、再次校验后原子替换 `/usr/bin/hyz-router`，并同步目录；
6. 执行 `/etc/init.d/S81hyz-router start`，等待 `/run/hyz-router/ready`；
7. 执行双上行状态矩阵、Mihomo core crash/fail-open 和 ownership 测试；失败时通过 USB 恢复备份 ELF 并重新启动。

`S81hyz-router stop` 的 runtime 清理等待上限为 130 秒，`start` 的 readiness 等待上限为 300 秒。停止期间板端网络可能完全不可用，但 USB ADB 应保持可用。

### 网络 ADB：原子替换、reboot

网络 ADB 不能先执行 stop，因为 router 的 shutdown 会清理管理 LAN、上游地址、bridge 和路由，导致 ADB transport 断开。网络路径必须在旧 router 仍运行时完成上传和替换：

1. 执行 `make router-app` 构建候选 ELF；
2. 在 router 仍运行时上传候选 ELF 到设备临时路径，核对主机和设备 SHA-256，并检查 wire protocol 兼容性；
3. 备份旧 `/usr/bin/hyz-router` 并校验备份 SHA-256；
4. 将候选文件写入 `.next`、校验后使用原子 `mv` 替换 `/usr/bin/hyz-router`，并同步目录；当前运行中的旧进程继续使用已加载的旧 ELF；
5. 执行 `adb reboot`，让启动流程加载新的 `/usr/bin/hyz-router`；
6. 等待选定的网络 ADB 恢复，再核对新 ELF SHA-256、router readiness 和完整状态；
7. 执行双上行状态矩阵、Mihomo core crash/fail-open 和 ownership 测试。

网络 ADB 热替换只适合已经通过主机验证、候选 ELF 已完成协议和 SHA-256 检查，并且现场存在 USB ADB、串口或其他物理恢复通道的开发验证。新 ELF 启动失败时，网络 ADB 可能无法恢复，不能把网络 ADB 热替换当作无条件可回滚的远程发布流程。网络 ADB 可以使用物理 LAN 或 Tailscale 地址，但必须在操作前确认选定路径和 TCP 5555 可达；无论使用哪种网络路径，都不能先 stop。

两条路径都必须保留旧 ELF 备份，不得直接覆盖写正在使用的目标文件，也不得依赖 `deploy-app.sh`、bind mount 或临时 init 脚本绕过 router 的 ownership/readiness 事务。

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
- 通过 ADB 直接替换或启动 `hyz-router` 作为正式发布方式；开发板临时验证必须遵循上面的 USB 或网络 ADB 流程；
- 通过 `deploy-app.sh` 热推送 router ELF；
- 通过临时 init 脚本绕过 OTA 或 router 的 ownership/readiness 事务。

`apps/rust/things/tools/deploy-app.sh` 只允许热部署 `hyz-things` 和 `hyz-camera`。这两个应用的热部署不得停止或重启 router。router 的开发热替换使用本文件规定的独立流程；router 正式发布和 wire contract 的正式版本变更仍必须通过 OTA 一起发布。

## 非 router 应用热部署

构建并热推送单个应用：

```sh
ADB_SERIAL=<serial> make deploy-things
ADB_SERIAL=<serial> make deploy-camera
```

也可以对已有 ELF 直接调用工具：

```sh
ADB_SERIAL=<serial> bash apps/rust/things/tools/deploy-app.sh deploy things <hyz-things-ELF>
ADB_SERIAL=<serial> bash apps/rust/things/tools/deploy-app.sh deploy camera <hyz-camera-ELF>
```

工具会先做 wire protocol 兼容性检查，再校验 ELF SHA-256，只停止并重启对应的 `S83hyz-things` 或 `S82hyz-camera`，最后更新 `/userdata/hyz-things/apps/registry.json`；router 的 ready 标记必须保持不变。回滚使用 `make revert-things` 或 `make revert-camera`。网络 ADB 需要同时设置 `ADB="adb -P 5038"` 和 `ADB_SERIAL`，详见 [`network-adb.md`](network-adb.md)。

## 开发验证边界

主机侧继续使用 crate 级格式化、clippy、host 测试和静态脚本检查。真实设备上的开发板验证可以使用上面的受控 binary staging、stop/start 或 reboot 流程，但仍必须通过状态矩阵和 cleanup 复核；普通 host 测试不执行目标设备上的网络、iptables、`/run`、`/userdata` 或进程状态修改。
