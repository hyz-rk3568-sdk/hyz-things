SHELL := /bin/bash
.DEFAULT_GOAL := upgrade

BOARD := hyz_things_rk3568
BR_BOARD := rockchip_hyz_things
RECOVERY_BR_BOARD := rockchip_rk3568_recovery
BR_OUT := $(CURDIR)/sdk/buildroot/output/$(BR_BOARD)
RECOVERY_BR_OUT := $(CURDIR)/sdk/buildroot/output/$(RECOVERY_BR_BOARD)
BR_HOST := $(BR_OUT)/host
TOOLCHAIN_PREFIX := $(BR_HOST)/bin/aarch64-buildroot-linux-gnu-
RUST_TARGET := aarch64-unknown-linux-gnu
OUTPUT := $(CURDIR)/output
OVERLAY := $(OUTPUT)/rootfs-overlay
FLUTTER_ELINUX := $(CURDIR)/.tools/flutter-elinux/bin/flutter-elinux
FLUTTER_APP := $(CURDIR)/apps/flutter/hello_world
FLUTTER_BUNDLE := $(FLUTTER_APP)/build/elinux/arm64/release/bundle
LLVM_SHIM := $(CURDIR)/.tools/llvm
REPO := $(CURDIR)/.tools/repo
MANIFEST_URL ?= https://github.com/hyz-rk3568-sdk/manifests.git
MANIFEST ?= hyz-things.xml
RECOVERY_PACKAGE := package-file-hyz-ota-with-recovery
JOBS ?= $(shell nproc)

# Buildroot rejects whitespace in PATH. Keep builds independent from WSL's
# injected Windows paths and unrelated user toolchains.
BUILD_PATH := $(BR_HOST)/bin:$(HOME)/.cargo/bin:$(HOME)/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH := $(BUILD_PATH)
export RK_TOOLCHAIN_PREFIX := $(TOOLCHAIN_PREFIX)

.PHONY: help sdk configure toolchain rust flutter apps overlay rootfs kernel loader recovery firmware upgrade upgrade-recovery check clean

help:
	@printf '%s\n' \
	  'make sdk        Clone/sync the pinned SDK manifest' \
	  'make toolchain  Configure Buildroot and build the AArch64 toolchain' \
	  'make apps       Build Rust hello, Rust OTA, and Flutter hello' \
	  'make rootfs     Stage applications and build the Buildroot rootfs' \
	  'make kernel     Build the RK3568 kernel with the Buildroot compiler' \
	  'make recovery   Build the source-controlled recovery image' \
	  'make firmware   Build loader, kernel, recovery, rootfs, and partition images' \
	  'make upgrade    Build the normal OTA without recovery (default)' \
	  'make upgrade-recovery  Also build an explicit OTA containing recovery' \
	  'make check      Run fast static/unit/configuration checks'

$(REPO):
	@mkdir -p "$(dir $(REPO))"
	curl --fail --location --proto '=https' --tlsv1.2 \
	  https://storage.googleapis.com/git-repo-downloads/repo -o "$(REPO).tmp"
	chmod 0755 "$(REPO).tmp"
	mv "$(REPO).tmp" "$(REPO)"

sdk/build.sh: | $(REPO)
	@mkdir -p sdk
	cd sdk && "$(REPO)" init -u "$(MANIFEST_URL)" -m "$(MANIFEST)"
	cd sdk && "$(REPO)" sync -c -j"$(JOBS)"
	@test -x $@

sdk: $(REPO)
	@if [[ ! -d sdk/.repo ]]; then \
	  mkdir -p sdk; \
	  cd sdk && "$(REPO)" init -u "$(MANIFEST_URL)" -m "$(MANIFEST)"; \
	fi
	cd sdk && "$(REPO)" sync -c -j"$(JOBS)"

configure: sdk/build.sh
	cd sdk && ./build.sh $(BOARD)_defconfig

toolchain: configure
	cd sdk && ./build.sh buildroot-make:toolchain:host-flex:host-lz4:host-dtc
rust: toolchain
	rustup target add $(RUST_TARGET)
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$(TOOLCHAIN_PREFIX)gcc" \
	CC_aarch64_unknown_linux_gnu="$(TOOLCHAIN_PREFIX)gcc" \
	  cargo build --locked --release --target $(RUST_TARGET) \
	  --manifest-path apps/rust/hello_world/Cargo.toml
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$(TOOLCHAIN_PREFIX)gcc" \
	CC_aarch64_unknown_linux_gnu="$(TOOLCHAIN_PREFIX)gcc" \
	  cargo build --locked --release --target $(RUST_TARGET) \
	  --manifest-path apps/rust/ota/Cargo.toml

$(FLUTTER_ELINUX):
	@mkdir -p .tools
	git clone --branch 3.27.1 --depth 1 \
	  https://github.com/sony/flutter-elinux.git .tools/flutter-elinux
	"$(FLUTTER_ELINUX)" config --no-analytics

flutter: toolchain $(FLUTTER_ELINUX)
	@mkdir -p "$(LLVM_SHIM)/bin" "$(OUTPUT)/logs"
	@CLANG=$$(command -v clang-20 || command -v clang); \
	 CXX=$$(command -v clang++-20 || command -v clang++); \
	 LLD=$$(command -v ld.lld-20 || command -v ld.lld); \
	 test -n "$$CLANG" -a -n "$$CXX" -a -n "$$LLD" || \
	   { echo 'clang, clang++, and ld.lld are required.' >&2; exit 1; }; \
	 ln -sfn "$$CLANG" "$(LLVM_SHIM)/bin/clang"; \
	 ln -sfn "$$CXX" "$(LLVM_SHIM)/bin/clang++"; \
	 ln -sfn "$$LLD" "$(LLVM_SHIM)/bin/ld.lld"
	cd "$(FLUTTER_APP)" && "$(FLUTTER_ELINUX)" build elinux --release \
	  --target-arch=arm64 \
	  --target-backend-type=wayland \
	  --target-compiler-triple=aarch64-buildroot-linux-gnu \
	  --target-sysroot="$(BR_HOST)/aarch64-buildroot-linux-gnu/sysroot" \
	  --target-toolchain="$(LLVM_SHIM)" \
	  --target-compiler-flags="--gcc-toolchain=$(BR_HOST) -fuse-ld=lld -Wno-unused-command-line-argument" \
	  2>&1 | tee "$(OUTPUT)/logs/flutter-build-arm64-wayland.log"

apps: rust flutter

overlay: apps
	rm -rf "$(OVERLAY)"
	mkdir -p "$(OVERLAY)"
	cp -a product/rootfs-overlay/. "$(OVERLAY)/"
	install -D -m 0755 \
	  apps/rust/hello_world/target/$(RUST_TARGET)/release/hyz-rust-hello \
	  "$(OVERLAY)/usr/bin/hyz-rust-hello"
	install -D -m 0755 \
	  apps/rust/ota/target/$(RUST_TARGET)/release/hyz-ota \
	  "$(OVERLAY)/usr/bin/hyz-ota"
	mkdir -p "$(OVERLAY)/opt/hyz/flutter/current"
	cp -a "$(FLUTTER_BUNDLE)/." "$(OVERLAY)/opt/hyz/flutter/current/"
	chmod 0755 "$(OVERLAY)/etc/init.d/S95hyz-app"
	printf 'hyz_things %s\n' "$${VERSION:-development}" > "$(OVERLAY)/etc/hyz-version"

rootfs: configure overlay
	cd sdk && ./build.sh buildroot

kernel: toolchain configure
	cd sdk && RK_TOOLCHAIN_PREFIX="$(TOOLCHAIN_PREFIX)" ./build.sh kernel

loader: toolchain configure
	cd sdk && RK_TOOLCHAIN_PREFIX="$(TOOLCHAIN_PREFIX)" ./build.sh loader

recovery: kernel
	cd sdk && RK_TOOLCHAIN_PREFIX="$(TOOLCHAIN_PREFIX)" ./build.sh recovery
	mkdir -p "$(OUTPUT)"
	cp -L sdk/output/recovery/recovery.img "$(OUTPUT)/recovery.img"
	cd "$(OUTPUT)" && sha256sum recovery.img > recovery.img.sha256

firmware: loader kernel rootfs recovery
	cd sdk && RK_TOOLCHAIN_PREFIX="$(TOOLCHAIN_PREFIX)" ./build.sh firmware

upgrade: firmware
	cd sdk && RK_TOOLCHAIN_PREFIX="$(TOOLCHAIN_PREFIX)" ./build.sh updateimg
	mkdir -p "$(OUTPUT)"
	cp -L sdk/rockdev/update.img "$(OUTPUT)/upgrade.fw"
	cd "$(OUTPUT)" && sha256sum upgrade.fw > upgrade.fw.sha256
	@printf 'Firmware: %s\nSHA-256: ' "$(OUTPUT)/upgrade.fw"
	@cut -d' ' -f1 "$(OUTPUT)/upgrade.fw.sha256"

upgrade-recovery: upgrade
	cd sdk && RK_TOOLCHAIN_PREFIX="$(TOOLCHAIN_PREFIX)" \
	  ./build.sh updateimg:$(RECOVERY_PACKAGE)
	mkdir -p "$(OUTPUT)"
	cp -L sdk/rockdev/update.img "$(OUTPUT)/upgrade-recovery.fw"
	cd "$(OUTPUT)" && sha256sum upgrade-recovery.fw > upgrade-recovery.fw.sha256
	@printf 'Recovery firmware: %s\nSHA-256: ' "$(OUTPUT)/upgrade-recovery.fw"
	@cut -d' ' -f1 "$(OUTPUT)/upgrade-recovery.fw.sha256"

check:
	cargo fmt --manifest-path apps/rust/hello_world/Cargo.toml -- --check
	cargo fmt --manifest-path apps/rust/ota/Cargo.toml -- --check
	cargo test --locked --manifest-path apps/rust/hello_world/Cargo.toml
	cargo test --locked --manifest-path apps/rust/ota/Cargo.toml
	sh -n product/rootfs-overlay/etc/init.d/S95hyz-app
	grep -q '^WIDTH=1080$$' product/rootfs-overlay/etc/init.d/S95hyz-app
	grep -q '^HEIGHT=1920$$' product/rootfs-overlay/etc/init.d/S95hyz-app
	grep -q 'wait_for_wayland' product/rootfs-overlay/etc/init.d/S95hyz-app
	sh -n sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	bash -n sdk/device/rockchip/common/post-hooks/20-info.sh
	grep -q '^RK_KERNEL_DTS_NAME="rk3568-atk-evb1-mipi-dsi-1080p"$$' sdk/device/rockchip/.chips/rk3566_rk3568/hyz_things_rk3568_defconfig
	grep -q '^RK_PACKAGE_FILE="package-file-hyz-ota"$$' sdk/device/rockchip/.chips/rk3566_rk3568/hyz_things_rk3568_defconfig
	grep -q '^RK_RECOVERY_BASE_CFG="rk3568"$$' sdk/device/rockchip/.chips/rk3566_rk3568/hyz_things_rk3568_defconfig
	! grep -q '^recovery[[:space:]]' sdk/device/rockchip/.chips/rk3566_rk3568/package-file-hyz-ota
	! grep -q '^userdata[[:space:]]' sdk/device/rockchip/.chips/rk3566_rk3568/package-file-hyz-ota
	grep -q '^recovery[[:space:]]*recovery.img$$' sdk/device/rockchip/.chips/rk3566_rk3568/$(RECOVERY_PACKAGE)
	! grep -q '^userdata[[:space:]]' sdk/device/rockchip/.chips/rk3566_rk3568/$(RECOVERY_PACKAGE)
	test -f sdk/external/recovery/Makefile
	test "$$(grep -c '<project ' sdk/.repo/manifests/hyz-things.xml)" -eq 20
	test "$$(grep -c '<project ' sdk/.repo/manifests/hyz-things-release.xml)" -eq 20
	test "$$(grep -cE 'revision="[0-9a-f]{40}"' sdk/.repo/manifests/hyz-things-release.xml)" -eq 20
	grep -q 'name="linux-external-recovery" path="external/recovery"' sdk/.repo/manifests/hyz-things.xml
	grep -q 'name="linux-external-recovery" path="external/recovery"' sdk/.repo/manifests/hyz-things-release.xml
	$(MAKE) -C sdk/buildroot O="$(BR_OUT)" $(BR_BOARD)_defconfig
	grep -q '^BR2_PACKAGE_RKUPDATE=y' "$(BR_OUT)/.config"
	grep -q '^BR2_PACKAGE_WESTON=y' "$(BR_OUT)/.config"
	! grep -qE '^BR2_PACKAGE_CHROMIUM.*=y$$' "$(BR_OUT)/.config"
	! grep -qE '^BR2_PACKAGE_RKNPU.*=y$$' "$(BR_OUT)/.config"
	! grep -qE '^BR2_PACKAGE_(FIBOCOM_DIAL_TOOL|QUECTEL_QCONNECTMANAGER)=y$$' "$(BR_OUT)/.config"
	$(MAKE) -C sdk/buildroot O="$(RECOVERY_BR_OUT)" $(RECOVERY_BR_BOARD)_defconfig
	grep -q '^BR2_PACKAGE_RECOVERY=y$$' "$(RECOVERY_BR_OUT)/.config"
	grep -q '^BR2_PACKAGE_RKUPDATE=y$$' "$(RECOVERY_BR_OUT)/.config"
	! grep -qE '^BR2_PACKAGE_(FIBOCOM_DIAL_TOOL|QUECTEL_QCONNECTMANAGER)=y$$' "$(RECOVERY_BR_OUT)/.config"

clean:
	rm -rf "$(OVERLAY)" \
	  "$(OUTPUT)/recovery.img" "$(OUTPUT)/recovery.img.sha256" \
	  "$(OUTPUT)/upgrade.fw" "$(OUTPUT)/upgrade.fw.sha256" \
	  "$(OUTPUT)/upgrade-recovery.fw" "$(OUTPUT)/upgrade-recovery.fw.sha256"
