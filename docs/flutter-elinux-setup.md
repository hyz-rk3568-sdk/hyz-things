# Sony flutter-elinux 初始化与 Hello World

本文只说明两件事：

1. 在 `hyz_things` 工作区初始化固定版本的 Sony `flutter-elinux`。
2. 创建并交叉构建一个面向 RK3568 的 Arm64 Wayland Flutter Hello World。

## 1. 固定版本

当前工作区固定使用以下版本：

| 组件 | 版本或提交 |
| --- | --- |
| Sony `flutter-elinux` | tag `3.27.1` |
| Sony 仓库提交 | `cbc86f6e71e26f20e0f823549d81e1f3cf4a26ae` |
| Flutter | `3.27.1` |
| Flutter Framework | `17025dd88227cd9532c33fa78f5250d548d87e9a` |
| Flutter Engine | `cb4b5fff73850b2e42bd4de7cb9a4310a78ac40d` |
| Dart | `3.6.0` |

不要单独执行 `flutter upgrade`。Sony 工具、Flutter Framework、Engine 和 Dart 需要保持版本匹配。

## 2. 安装位置

工具统一放在工作区本地目录：

```text
~/Code/hyz_things/.tools/flutter-elinux/
```

不建议移动到 `/opt`：

- 工作区脚本使用项目本地的固定版本。
- `.tools/` 已被工作区 `.gitignore` 忽略。
- 不需要处理 `/opt` 下的 root 权限和文件归属。
- 多个项目可以分别固定自己的 Flutter 版本。

最终目录关系如下：

```text
hyz_things/
├── .tools/
│   ├── flutter-elinux/          # Sony flutter-elinux
│   │   └── flutter/             # 与 Sony 版本匹配的 Flutter SDK
│   └── llvm/                    # 本地 Clang 命令 shim
├── apps/flutter/hello_world/
├── Makefile                     # 产品统一构建入口
└── output/logs/
```

## 3. 主机依赖

需要以下主机工具：

- Git
- curl
- unzip
- Clang 和 Clang++
- LLD
- CMake
- Make
- Ninja
- pkg-config

Ubuntu 通用安装命令：

```sh
sudo apt update
sudo apt install \
  git curl unzip \
  clang lld \
  cmake make ninja-build pkg-config
```

如果希望 `flutter-elinux doctor` 检测到主机 EGL/GLES 开发环境，可额外安装：

```sh
sudo apt install libegl1-mesa-dev libgles2-mesa-dev
```

当前 `hyz_things` 构建脚本使用：

```text
/usr/bin/clang-20
/usr/bin/clang++-20
/usr/bin/ld.lld-20
```

并在 `.tools/llvm/bin/` 下创建工作区本地 shim，不修改系统命令。

## 4. 克隆 Sony flutter-elinux

进入工作区：

```sh
cd ~/Code/hyz_things
mkdir -p .tools output/logs
```

克隆固定 tag：

```sh
git clone \
  --branch 3.27.1 \
  --depth 1 \
  https://github.com/sony/flutter-elinux.git \
  .tools/flutter-elinux
```

Sony 工具会读取：

```text
.tools/flutter-elinux/bin/internal/flutter.version
```

并把匹配的 Flutter SDK 放到：

```text
.tools/flutter-elinux/flutter/
```

## 5. 配置 zsh

构建脚本会直接定位工作区工具，因此不修改 `PATH` 也能使用。

如果需要在终端中直接运行 `flutter-elinux` 和 `flutter`，在 `~/.zshrc` 中加入：

```zsh
# hyz_things 固定版本 Sony flutter-elinux 和 Flutter SDK
export FLUTTER_ELINUX_HOME="$HOME/Code/hyz_things/.tools/flutter-elinux"
export PATH="$FLUTTER_ELINUX_HOME/bin:$FLUTTER_ELINUX_HOME/flutter/bin:$PATH"

# Flutter 和 Dart 国内镜像
export FLUTTER_STORAGE_BASE_URL="https://storage.flutter-io.cn"
export PUB_HOSTED_URL="https://pub.flutter-io.cn"
```

删除或注释旧路径，避免选择到另一套 Flutter SDK：

```zsh
# export PATH=/opt/flutter-elinux/bin:$PATH
# export PATH=/opt/flutter-elinux/flutter/bin:$PATH
# export PATH=$HOME/flutter_sdk/flutter/bin:$PATH
```

重新加载：

```sh
source ~/.zshrc
rehash
```

## 6. 首次初始化

保持正常的 HTTP/HTTPS 下载代理，然后运行：

```sh
cd ~/Code/hyz_things

flutter-elinux --version \
  2>&1 | tee output/logs/flutter-elinux-bootstrap.log

flutter-elinux config --no-analytics
```

首次运行可能下载：

- 固定提交的 Flutter SDK。
- Dart SDK。
- Flutter tool 的 Dart package。
- Flutter tool snapshot。

第一次构建 eLinux 应用时，还会从 Sony `flutter-embedded-linux` GitHub Release 下载与 Engine revision 匹配的：

- `elinux-common` artifacts。
- `elinux-arm64-release` Engine 和 Wayland embedder。
- 其他由当前 Sony CLI 预缓存的 eLinux artifacts。

## 7. 代理导致初始化卡住时的处理

在当前 WSL 环境中，Flutter tool snapshot 初始化曾停在：

```text
Building flutter tool...
Resolving dependencies...
Downloading packages...
Got dependencies.
```

原因是初始化期间访问 Google Cloud metadata 地址 `169.254.169.254`，请求被本机代理 `127.0.0.1:7890` 接管后没有结束。

只有遇到这个现象时才使用下面的恢复方法。先中断卡住的命令，但不要删除已经下载的 Dart SDK 和 package cache。

设置路径：

```sh
cd ~/Code/hyz_things

FEL="$PWD/.tools/flutter-elinux"
FLUTTER="$FEL/flutter"
DART="$FLUTTER/bin/cache/dart-sdk/bin/dart"
TOOLS="$FLUTTER/packages/flutter_tools"
CLOSED_PROXY=http://127.0.0.1:9
```

使用一个拒绝连接的本地端口生成 Flutter tool snapshot，使 metadata 探测立即失败：

```sh
rm -f "$FLUTTER/bin/cache/flutter_tools.snapshot"

env \
  http_proxy="$CLOSED_PROXY" \
  https_proxy="$CLOSED_PROXY" \
  all_proxy="$CLOSED_PROXY" \
  HTTP_PROXY="$CLOSED_PROXY" \
  HTTPS_PROXY="$CLOSED_PROXY" \
  ALL_PROXY="$CLOSED_PROXY" \
  "$DART" \
    --verbosity=error \
    --snapshot="$FLUTTER/bin/cache/flutter_tools.snapshot" \
    --snapshot-kind=app-jit \
    --packages="$TOOLS/.dart_tool/package_config.json" \
    --no-enable-mirrors \
    "$TOOLS/bin/flutter_tools.dart"
```

写入 Flutter tool stamp：

```sh
printf '%s:\n' "$(git -C "$FLUTTER" rev-parse HEAD)" \
  > "$FLUTTER/bin/cache/flutter_tools.stamp"
```

关闭 analytics：

```sh
env \
  http_proxy="$CLOSED_PROXY" \
  https_proxy="$CLOSED_PROXY" \
  all_proxy="$CLOSED_PROXY" \
  HTTP_PROXY="$CLOSED_PROXY" \
  HTTPS_PROXY="$CLOSED_PROXY" \
  ALL_PROXY="$CLOSED_PROXY" \
  "$FLUTTER/bin/flutter" --disable-analytics
```

使用正常下载代理解析 Sony CLI 依赖：

```sh
cd "$FEL"
./flutter/bin/flutter pub upgrade
```

然后使用相同的 closed proxy 编译 Sony CLI snapshot：

```sh
rm -f bin/cache/flutter-elinux.snapshot

env \
  http_proxy="$CLOSED_PROXY" \
  https_proxy="$CLOSED_PROXY" \
  all_proxy="$CLOSED_PROXY" \
  HTTP_PROXY="$CLOSED_PROXY" \
  HTTPS_PROXY="$CLOSED_PROXY" \
  ALL_PROXY="$CLOSED_PROXY" \
  ./flutter/bin/cache/dart-sdk/bin/dart \
    --disable-dart-dev \
    --no-enable-mirrors \
    --snapshot=bin/cache/flutter-elinux.snapshot \
    --packages=.dart_tool/package_config.json \
    bin/flutter_elinux.dart

git rev-parse HEAD > bin/cache/flutter-elinux.stamp
```

`CLOSED_PROXY` 只用于本地 snapshot 编译，不要写进 `~/.zshrc`。下载 Flutter、Dart package 和 Sony Engine artifacts 时仍然使用正常代理。

## 8. 创建 Flutter Hello World

从工作区根目录执行：

```sh
cd ~/Code/hyz_things

flutter-elinux create \
  --platforms=elinux \
  --project-name=hyz_flutter_hello \
  --org=dev.hyz.things \
  apps/flutter/hello_world
```

关键参数：

- `--platforms=elinux`：只生成 Sony eLinux runner，不生成 Android、iOS、Web 等目录。
- `--project-name=hyz_flutter_hello`：Dart package 和目标可执行文件名称。
- `--org=dev.hyz.things`：应用组织标识。

主要目录：

```text
apps/flutter/hello_world/
├── elinux/             # Sony eLinux CMake runner
├── lib/main.dart       # Flutter 应用入口
├── test/               # Widget test
├── pubspec.yaml
└── pubspec.lock
```

## 9. 编写 Hello World 页面

将 `apps/flutter/hello_world/lib/main.dart` 修改为：

```dart
import 'package:flutter/material.dart';

void main() {
  runApp(const HyzHelloApp());
}

class HyzHelloApp extends StatelessWidget {
  const HyzHelloApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'hyz_things Flutter Hello',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xff4f46e5),
          brightness: Brightness.dark,
        ),
        useMaterial3: true,
      ),
      home: const Scaffold(
        body: Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.memory, size: 88),
              SizedBox(height: 24),
              Text(
                'Hello from Flutter!',
                style: TextStyle(
                  fontSize: 40,
                  fontWeight: FontWeight.bold,
                ),
              ),
              SizedBox(height: 12),
              Text(
                'hyz_things · RK3568 · Wayland',
                style: TextStyle(fontSize: 22),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
```

## 10. Buildroot 交叉工具链

Flutter 的 Dart AOT 部分由 Flutter SDK 生成，eLinux 原生 runner 使用 Buildroot AArch64 glibc sysroot 进行交叉编译。

工具链目录：

```text
sdk/buildroot/output/rockchip_atk_dlrk3568/host/
```

Sysroot：

```text
sdk/buildroot/output/rockchip_atk_dlrk3568/host/aarch64-buildroot-linux-gnu/sysroot/
```

这里只需要已经生成的 Buildroot `toolchain`，不需要编译 kernel、rootfs、U-Boot 或完整固件。

## 11. 构建 Arm64 Wayland Hello World

工作区已经提供构建脚本：

```sh
cd ~/Code/hyz_things
make flutter
```

脚本对应的核心命令是：

```sh
flutter-elinux build elinux --release \
  --target-arch=arm64 \
  --target-backend-type=wayland \
  --target-compiler-triple=aarch64-buildroot-linux-gnu \
  --target-sysroot="$SYSROOT" \
  --target-toolchain="$LLVM_SHIM" \
  --target-compiler-flags="--gcc-toolchain=$BUILDROOT_HOST -fuse-ld=lld -Wno-unused-command-line-argument"
```

参数含义：

- `--release`：生成 Dart AOT release bundle。
- `--target-arch=arm64`：目标架构为 RK3568 AArch64。
- `--target-backend-type=wayland`：使用 Weston/Wayland backend。
- `--target-compiler-triple`：指定 Buildroot AArch64 target triple。
- `--target-sysroot`：使用 Buildroot 生成的 glibc sysroot。
- `--target-toolchain`：使用工作区内的 Clang shim。
- `--gcc-toolchain`：让 Clang 查找 Buildroot GCC runtime 和 C++ 标准库。
- `-fuse-ld=lld`：使用支持 AArch64 的 LLD 链接器。

构建输出目录：

```text
apps/flutter/hello_world/build/elinux/arm64/release/bundle/
```

bundle 的主要内容：

```text
bundle/
├── hyz_flutter_hello
├── data/
│   ├── icudtl.dat
│   └── flutter_assets/
└── lib/
    ├── libapp.so
    ├── libflutter_engine.so
    └── libflutter_elinux_wayland.so
```

其中：

- `hyz_flutter_hello`：Sony eLinux 原生 runner。
- `libapp.so`：Hello World 的 Dart AOT 代码。
- `libflutter_engine.so`：Flutter Arm64 Engine。
- `libflutter_elinux_wayland.so`：Sony Wayland embedder。
- `flutter_assets/`：字体、资源和 Flutter asset manifest。
