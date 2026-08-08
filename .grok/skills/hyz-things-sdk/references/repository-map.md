# SDK repository map

The SDK workspace contains 19 private component repositories. Paths are relative to `sdk/`.

## Core

| Path | GitHub repository | Owns |
|---|---|---|
| `buildroot` | `linux-buildroot` | Toolchain, rootfs, packages, overlays, Weston, Qt, networking userspace |
| `device/rockchip` | `linux-device-rockchip` | Board selection, build hooks/scripts, image orchestration, Wi-Fi post-rootfs integration |
| `kernel` | `rk-kernel` | Linux 5.10 kernel, DTS, defconfig fragments, drivers |
| `u-boot` | `android-rk-u-boot` | U-Boot and boot flow |
| `rkbin` | `rk-rkbin` | Rockchip DDR/TPL/BL31 binaries and loader data |
| `external/rkwifibt` | `linux-external-rkwifibt` | RTL8852BS and other vendor Wi-Fi/Bluetooth source/firmware |
| `external/rktoolkit` | `linux-external-rktoolkit` | Rockchip low-level userspace helpers used by the base rootfs |
| `external/rkscript` | `linux-external-rkscript` | Mount, USB gadget, udev, and boot helper services |
| `external/rkupdate` | `linux-external-rkupdate` | Rockchip update image parser and target-side writer |
| `tools` | `linux-tools` | Minimal host-side `Linux_Pack_Firmware` tools only |

## Media

| Path | GitHub repository | Owns |
|---|---|---|
| `external/alsa-config` | `linux-external-alsa-config` | Rockchip ALSA configuration |
| `external/camera_engine_rkaiq` | `linux-external-camera_engine_rkaiq` | RKAIQ camera stack |
| `external/common_algorithm` | `linux-external-common_algorithm` | Rockchip common audio/video algorithms |
| `external/gstreamer-rockchip` | `linux-gstreamer-rockchip` | Rockchip GStreamer integration |
| `external/libmali` | `linux-libmali` | Mali userspace GPU libraries |
| `external/linux-rga` | `linux-linux-rga` | RGA acceleration library |
| `external/rockit` | `linux-rockit` | Rockit media framework |
| `external/mpp` | `rk-mpp` | Rockchip hardware codec framework |
| `app/rkadk` | `linux-bsp-rkadk` | RKADK application/media framework |

## Deliberately absent

The manifest does not include Yocto, Debian, Chromium, NPU, vendor tests/demos, top-level `prebuilts`, or unrelated vendor tools. The `tools` project is intentionally limited to `linux/Linux_Pack_Firmware`.

Consequences:

- The `mk-updateimg.sh` flow is supported; `rkflash.sh` and unrelated vendor flashing flows remain intentionally absent.
- Kernel and loader builds must use the Buildroot-generated compiler through `RK_TOOLCHAIN_PREFIX`; top-level prebuilt GCC is not restored.
- Product applications live in the separate `hyz-things` repository and are staged into the rootfs by its top-level Makefile.
