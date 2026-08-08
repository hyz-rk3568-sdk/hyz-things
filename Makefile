SHELL := /bin/bash
.DEFAULT_GOAL := upgrade

BOARD := hyz_things_rk3568
BR_BOARD := rockchip_hyz_things
BR_OUT := $(CURDIR)/sdk/buildroot/output/$(BR_BOARD)
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
JOBS ?= $(shell nproc)

# Buildroot rejects whitespace in PATH. Keep builds independent from WSL's
# injected Windows paths and unrelated user toolchains.
BUILD_PATH := $(BR_HOST)/bin:$(HOME)/.cargo/bin:$(HOME)/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH := $(BUILD_PATH)
export RK_TOOLCHAIN_PREFIX := $(TOOLCHAIN_PREFIX)

.PHONY: help sdk configure toolchain rust flutter apps overlay rootfs kernel loader firmware upgrade check clean

help:
	@printf '%s\n' \
	  'make sdk        Clone/sync the pinned SDK manifest' \
	  'make toolchain  Configure Buildroot and build the AArch64 toolchain' \
	  'make apps       Build Rust hello, Rust OTA, and Flutter hello' \
	  'make rootfs     Stage applications and build the Buildroot rootfs' \
	  'make kernel     Build the RK3568 kernel with the Buildroot compiler' \
	  'make firmware   Build loader, kernel, rootfs, and partition images' \
	  'make upgrade    Build output/upgrade.fw and its SHA-256 file (default)' \
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

firmware: loader kernel rootfs
	cd sdk && RK_TOOLCHAIN_PREFIX="$(TOOLCHAIN_PREFIX)" ./build.sh firmware

upgrade: firmware
	cd sdk && RK_TOOLCHAIN_PREFIX="$(TOOLCHAIN_PREFIX)" ./build.sh updateimg
	mkdir -p "$(OUTPUT)"
	cp -L sdk/rockdev/update.img "$(OUTPUT)/upgrade.fw"
	cd "$(OUTPUT)" && sha256sum upgrade.fw > upgrade.fw.sha256
	@printf 'Firmware: %s\nSHA-256: ' "$(OUTPUT)/upgrade.fw"
	@cut -d' ' -f1 "$(OUTPUT)/upgrade.fw.sha256"

check:
	cargo fmt --manifest-path apps/rust/hello_world/Cargo.toml -- --check
	cargo fmt --manifest-path apps/rust/ota/Cargo.toml -- --check
	cargo test --locked --manifest-path apps/rust/hello_world/Cargo.toml
	cargo test --locked --manifest-path apps/rust/ota/Cargo.toml
	sh -n product/rootfs-overlay/etc/init.d/S95hyz-app
	$(MAKE) -C sdk/buildroot O="$(BR_OUT)" $(BR_BOARD)_defconfig
	grep -q '^BR2_PACKAGE_RKUPDATE=y' "$(BR_OUT)/.config"
	grep -q '^BR2_PACKAGE_WESTON=y' "$(BR_OUT)/.config"
	! grep -qE '^BR2_PACKAGE_CHROMIUM.*=y$$' "$(BR_OUT)/.config"
	! grep -qE '^BR2_PACKAGE_RKNPU.*=y$$' "$(BR_OUT)/.config"
	! grep -qE '^BR2_PACKAGE_(FIBOCOM_DIAL_TOOL|QUECTEL_QCONNECTMANAGER)=y$$' "$(BR_OUT)/.config"

clean:
	rm -rf "$(OVERLAY)" "$(OUTPUT)/upgrade.fw" "$(OUTPUT)/upgrade.fw.sha256"
