SHELL := /bin/bash
.DEFAULT_GOAL := upgrade

BOARD := hyz_things_rk3568
BR_BOARD := rockchip_hyz_things
BR_OUT := $(CURDIR)/sdk/buildroot/output/$(BR_BOARD)
BR_HOST := $(BR_OUT)/host
TOOLCHAIN_PREFIX := $(BR_HOST)/bin/aarch64-buildroot-linux-gnu-
RUST_TARGET := aarch64-unknown-linux-gnu
ROUTER_APP := $(CURDIR)/apps/rust/router
ROUTER_FRONTEND_BUNDLE := $(CURDIR)/target/frontend-bundle/router-frontend.tar
ROUTER_TRUNK := $(CURDIR)/.tools/trunk/bin/trunk
ROUTER_BINARY := $(ROUTER_APP)/target/$(RUST_TARGET)/release/hyz-router
OUTPUT := $(CURDIR)/output
OVERLAY := $(OUTPUT)/rootfs-overlay
REPO := $(CURDIR)/.tools/repo
MANIFEST_URL ?= https://github.com/hyz-rk3568-sdk/manifests.git
MANIFEST ?= hyz-things.xml
RECOVERY_PACKAGE := package-file-hyz-ota-with-recovery
ADB ?= adb
ADB_SERIAL ?=
JOBS ?= $(shell nproc)

# Buildroot rejects whitespace in PATH. Keep builds independent from WSL's
# injected Windows paths and unrelated user toolchains.
BUILD_PATH := $(BR_HOST)/bin:$(HOME)/.cargo/bin:$(HOME)/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH := $(BUILD_PATH)
export RK_TOOLCHAIN_PREFIX := $(TOOLCHAIN_PREFIX)

.PHONY: help sdk configure toolchain router-frontend router-e2e router-app router-deploy-dev router-revert-dev apps overlay rootfs kernel loader recovery firmware upgrade upgrade-recovery check check-static clean

help:
	@printf '%s\n' \
	  'make sdk        Clone/sync the pinned SDK manifest' \
	  'make toolchain  Configure Buildroot and build the AArch64 toolchain' \
	  'make router-frontend  Build the deterministic Yew/Tailwind frontend bundle' \
	  'make router-e2e  Run the host-only Axum/Playwright browser tests' \
	  'make router-app Build the unified hyz-router ELF and embedded Yew UI' \
	  'make router-deploy-dev  Build and activate only hyz-router over USB ADB' \
	  'make router-revert-dev  Gracefully restore the firmware hyz-router ELF' \
	  'make apps       Build the single product application (hyz-router)' \
	  'make overlay    Stage only hyz-router and product metadata' \
	  'make rootfs     Stage hyz-router and build the Buildroot rootfs' \
	  'make kernel     Build the RK3568 kernel with the Buildroot compiler' \
	  'make recovery   Build the source-controlled recovery image' \
	  'make firmware   Build loader, kernel, recovery, rootfs, and partition images' \
	  'make upgrade    Build the normal OTA without recovery (default)' \
	  'make upgrade-recovery  Also build an explicit OTA containing recovery' \
	  'make check-static  Run source/configuration checks without compiling' \
	  'make check      Run router formatting, tests, strict Clippy, and static checks'

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

router-frontend:
	test -x "$(ROUTER_TRUNK)" || { echo 'repository-local Trunk 0.21.14 is required under .tools/trunk.' >&2; exit 1; }
	test -x "$(ROUTER_APP)/node_modules/.bin/tailwindcss" || { echo 'run npm ci in apps/rust/router first.' >&2; exit 1; }
	rustup target add wasm32-unknown-unknown
	PATH="$(dir $(ROUTER_TRUNK)):$(PATH)" bash "$(ROUTER_APP)/tools/build-frontend-bundle.sh"
	test -s "$(ROUTER_FRONTEND_BUNDLE)"

router-e2e:
	test -x "$(ROUTER_TRUNK)" || { echo 'repository-local Trunk 0.21.14 is required under .tools/trunk.' >&2; exit 1; }
	cd "$(ROUTER_APP)" && PATH="$(dir $(ROUTER_TRUNK)):$(PATH)" npm run test:e2e

router-app: toolchain router-frontend
	rustup target add $(RUST_TARGET)
	ROUTER_FRONTEND_ARCHIVE="$(ROUTER_FRONTEND_BUNDLE)" \
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$(TOOLCHAIN_PREFIX)gcc" \
	CC_aarch64_unknown_linux_gnu="$(TOOLCHAIN_PREFIX)gcc" \
	  cargo build --locked --release --target $(RUST_TARGET) \
	  --manifest-path "$(ROUTER_APP)/Cargo.toml" \
	  --bin hyz-router --features native

apps: router-app

router-deploy-dev: router-app
	ADB="$(ADB)" ADB_SERIAL="$(ADB_SERIAL)" bash "$(ROUTER_APP)/tools/deploy-dev.sh" deploy "$(ROUTER_BINARY)"

router-revert-dev:
	ADB="$(ADB)" ADB_SERIAL="$(ADB_SERIAL)" bash "$(ROUTER_APP)/tools/deploy-dev.sh" revert

overlay: apps
	rm -rf "$(OVERLAY)"
	mkdir -p "$(OVERLAY)"
	cp -a product/rootfs-overlay/. "$(OVERLAY)/"
	install -D -m 0755 "$(ROUTER_BINARY)" "$(OVERLAY)/usr/bin/hyz-router"
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

check: check-static
	cargo fmt --manifest-path "$(ROUTER_APP)/Cargo.toml" --all -- --check
	cargo test --locked --manifest-path "$(ROUTER_APP)/Cargo.toml" --features native
	cargo clippy --locked --manifest-path "$(ROUTER_APP)/Cargo.toml" \
	  --all-targets --features native -- -D warnings

check-static:
	sh -n "$(ROUTER_APP)/tools/build-frontend-bundle.sh"
	bash -n "$(ROUTER_APP)/tools/deploy-dev.sh"
	grep -q 'remote_action stop' "$(ROUTER_APP)/tools/deploy-dev.sh"
	grep -q "mount -o bind" "$(ROUTER_APP)/tools/deploy-dev.sh"
	! grep -q 'rm -rf /run/hyz-router/daemon.lock\|kill -9\|pkill' "$(ROUTER_APP)/tools/deploy-dev.sh"
	sh -n "$(ROUTER_APP)/tools/start-e2e-server.sh"
	python3 -m json.tool "$(ROUTER_APP)/package.json" >/dev/null
	python3 -m json.tool "$(ROUTER_APP)/package-lock.json" >/dev/null
	grep -q '"lockfileVersion"' "$(ROUTER_APP)/package-lock.json"
	grep -q '@plugin "daisyui"' "$(ROUTER_APP)/frontend/app.css"
	grep -q '@source "../src/web/\*\*/\*.rs"' "$(ROUTER_APP)/frontend/app.css"
	python3 -c 'from pathlib import Path; p = Path("$(ROUTER_APP)/tools/externalize-trunk-bootstrap.py"); compile(p.read_bytes(), str(p), "exec")'
	grep -q 'router-bootstrap\.js' "$(ROUTER_APP)/tools/externalize-trunk-bootstrap.py"
	grep -q 'bootstrap_version' "$(ROUTER_APP)/tools/externalize-trunk-bootstrap.py"
	grep -q 'path == "router-bootstrap.js"' "$(ROUTER_APP)/src/adapters/inbound/http/mod.rs"
	grep -q 'static_asset_path' "$(ROUTER_APP)/src/adapters/inbound/http/mod.rs"
	! grep -R -q 'unsafe-inline' "$(ROUTER_APP)/src" "$(ROUTER_APP)/frontend" "$(ROUTER_APP)/tools"
	grep -q "script-src 'self' 'wasm-unsafe-eval'" "$(ROUTER_APP)/src/adapters/inbound/http/mod.rs"
	! grep -q "script-src 'self' 'unsafe-eval'" "$(ROUTER_APP)/src/adapters/inbound/http/mod.rs"
	grep -q 'Ipv4Addr::new(192, 168, 8, 1)' "$(ROUTER_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/control/display"' "$(ROUTER_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/control/proxy/delay"' "$(ROUTER_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/control/proxy/delays"' "$(ROUTER_APP)/src/adapters/inbound/http/mod.rs"
	grep -q 'MIHOMO_CONTROLLER_ADDRESS: &str = "127.0.0.1:9090"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	grep -q 'TAILSCALED_EXECUTABLE: &str = "/usr/bin/tailscaled"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	grep -q 'TAILSCALE_EXECUTABLE: &str = "/usr/bin/tailscale"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	grep -q 'TAILSCALE_STATE_FILE: &str = "/userdata/hyz-router/tailscale/tailscaled.state"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	grep -q 'TAILSCALE_SOCKET: &str = "/run/hyz-tailscale/tailscaled.sock"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	grep -q 'TAILSCALE_INTERFACE: &str = "tailscale0"' "$(ROUTER_APP)/src/domain/tailscale.rs"
	grep -q 'TAILSCALE_LAN_ROUTE: &str = "192.168.8.0/24"' "$(ROUTER_APP)/src/domain/tailscale.rs"
	grep -q 'TAILSCALE_UDP_PORT: u16 = 41_641' "$(ROUTER_APP)/src/domain/tailscale.rs"
	grep -q 'TAILSCALE_MANAGEMENT_HTTP_PORT: u16 = 8080' "$(ROUTER_APP)/src/domain/tailscale.rs"
	! grep -qE 'pub (login_server|auth_key|subnet|advertise_routes|exit_node|argv|tag):' \
	  "$(ROUTER_APP)/src/adapters/inbound/control.rs" \
	  "$(ROUTER_APP)/src/adapters/inbound/http/mod.rs"
	! grep -R -qE '127\.0\.0\.1:9090|controller\.secret' "$(ROUTER_APP)/src/web" "$(ROUTER_APP)/frontend"
	grep -q 'default-brightness-level = <0>' sdk/kernel/arch/arm64/boot/dts/rockchip/rk3568-atk-evb1-mipi-dsi-1080p.dts
	! grep -qE '&(dsi1|dsi1_panel|backlight1)[[:space:]]*\{[[:space:]]*status = "disabled"' sdk/kernel/arch/arm64/boot/dts/rockchip/rk3568-atk-evb1-mipi-dsi-1080p.dts
	! grep -R -E -q 'TcpListener::bind\([^)]*(UNSPECIFIED|\[0,[[:space:]]*0,[[:space:]]*0,[[:space:]]*0\])|Ipv4Addr::UNSPECIFIED|CorsLayer::permissive|/usr/sbin/hyz-mihomo' "$(ROUTER_APP)/src"
	! grep -R -q 'Command::new("sh")\|Command::new("bash")' "$(ROUTER_APP)/src"
	grep -q 'network-config-sta-rollback-v1.json' "$(ROUTER_APP)/src/adapters/outbound/network_config.rs"
	grep -q 'let cleanup_succeeded = cleanup.is_ok()' "$(ROUTER_APP)/src/main.rs"
	test ! -d "$(ROUTER_APP)/adapter-linux"
	sh -n sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'TARGET_DIR/usr/bin/hyz-ota' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'TARGET_DIR/usr/sbin/hyz-router' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'TARGET_DIR/etc/init.d/S82tailscaled' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'TARGET_DIR/usr/share/metacubexd' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	sh -n sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^DAEMON=/usr/bin/hyz-router$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^START_TIMEOUT_SECONDS=300$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^START_EXEC_GRACE_ATTEMPTS=10$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^START_RETRY_BACKOFF_MAX=30$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^STOP_CLEANUP_TIMEOUT_SECONDS=30$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^FLOCK=/usr/bin/flock$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^ACTION_LOCKFILE=/run/hyz-router-init.lock$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'lock_action && stop_daemon && start_daemon' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q -- '-- daemon 9>&-' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'deadline=.*START_TIMEOUT_SECONDS' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'daemon left running' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'wait_for_runtime_cleanup' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	! grep -q 'TERM/1810/KILL/5 || true' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'retrying after attempt' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	! grep -q 'START_LAUNCH_ATTEMPTS\|START_ATTEMPTS' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'stale ownership requires explicit recovery' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	! grep -qE 'hyz-mihomo (explicit|tun|disable)|hyz-mihomo removes' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/hyz-router/mihomo-config.yaml.example
	grep -q 'hyz-router proxy tun' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/hyz-router/mihomo-config.yaml.example
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-mihomo
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/usr/sbin/hyz-router
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/usr/sbin/hyz-mihomo
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/usr/share/udhcpc/default.script.d/50-hyz-wlan-metric
	grep -q 'BR2_ROOTFS_OVERLAY+="board/rockchip/hyz_things/fs-overlay ../../output/rootfs-overlay"' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_MIHOMO=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_TAILSCALE=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	sdk/buildroot/utils/check-package \
	  sdk/buildroot/package/mihomo/Config.in \
	  sdk/buildroot/package/mihomo/mihomo.mk \
	  sdk/buildroot/package/mihomo/mihomo.hash \
	  sdk/buildroot/package/tailscale/Config.in \
	  sdk/buildroot/package/tailscale/tailscale.mk \
	  sdk/buildroot/package/tailscale/tailscale.hash
	grep -q '^MIHOMO_VERSION = 1\.19\.29$$' sdk/buildroot/package/mihomo/mihomo.mk
	grep -q '^TAILSCALE_VERSION = 1\.102\.2$$' sdk/buildroot/package/tailscale/tailscale.mk
	grep -F -q '$$(TARGET_DIR)/usr/bin/tailscale' sdk/buildroot/package/tailscale/tailscale.mk
	grep -F -q '$$(TARGET_DIR)/usr/bin/tailscaled' sdk/buildroot/package/tailscale/tailscale.mk
	! grep -qE 'etc/init\.d|userdata|auth.?key|tailscale.*(web|config)' sdk/buildroot/package/tailscale/tailscale.mk
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82tailscaled
	@for symbol in BRIDGE TUN IP_ADVANCED_ROUTER IP_MULTIPLE_TABLES NF_CONNTRACK \
	  IP_NF_FILTER IP_NF_NAT IP_NF_TARGET_MASQUERADE \
	  NETFILTER_XT_TARGET_MARK NETFILTER_XT_MATCH_MARK NETFILTER_XT_MATCH_MAC \
	  NETFILTER_XT_MATCH_SOCKET NETFILTER_XT_MATCH_COMMENT NETFILTER_XT_TARGET_TPROXY \
	  NETFILTER_XT_TARGET_REDIRECT IP_NF_MANGLE; do \
		grep -q "^CONFIG_$$symbol=y$$" sdk/kernel/arch/arm64/configs/rockchip_linux_defconfig || exit 1; \
	done
	bash -n sdk/device/rockchip/common/post-hooks/20-info.sh
	grep -q '^RK_KERNEL_DTS_NAME="rk3568-atk-evb1-mipi-dsi-1080p"$$' sdk/device/rockchip/.chips/rk3566_rk3568/hyz_things_rk3568_defconfig
	grep -q '^RK_PACKAGE_FILE="package-file-hyz-ota"$$' sdk/device/rockchip/.chips/rk3566_rk3568/hyz_things_rk3568_defconfig
	grep -q '^RK_RECOVERY_BASE_CFG="rk3568"$$' sdk/device/rockchip/.chips/rk3566_rk3568/hyz_things_rk3568_defconfig
	grep -q '^BR2_PACKAGE_RECOVERY=y$$' sdk/buildroot/configs/rockchip/base/recovery.config
	grep -q '^BR2_PACKAGE_RKUPDATE=y$$' sdk/buildroot/configs/rockchip/base/recovery.config
	! grep -qE '^BR2_PACKAGE_(FIBOCOM_DIAL_TOOL|QUECTEL_QCONNECTMANAGER)=y$$' sdk/buildroot/configs/rockchip/base/recovery.config
	! grep -q '^recovery[[:space:]]' sdk/device/rockchip/.chips/rk3566_rk3568/package-file-hyz-ota
	! grep -q '^userdata[[:space:]]' sdk/device/rockchip/.chips/rk3566_rk3568/package-file-hyz-ota
	grep -q '^recovery[[:space:]]*recovery.img$$' sdk/device/rockchip/.chips/rk3566_rk3568/$(RECOVERY_PACKAGE)
	! grep -q '^userdata[[:space:]]' sdk/device/rockchip/.chips/rk3566_rk3568/$(RECOVERY_PACKAGE)
	test -f sdk/external/recovery/Makefile
	test "$$(grep -c '<project ' sdk/.repo/manifests/hyz-things.xml)" -eq 20
	test "$$(grep -c '<project ' sdk/.repo/manifests/hyz-things-release.xml)" -eq 20
	test "$$(grep -cE 'revision="[0-9a-f]{40}"' sdk/.repo/manifests/hyz-things-release.xml)" -eq 20

clean:
	rm -rf "$(OVERLAY)" \
	  "$(OUTPUT)/recovery.img" "$(OUTPUT)/recovery.img.sha256" \
	  "$(OUTPUT)/upgrade.fw" "$(OUTPUT)/upgrade.fw.sha256" \
	  "$(OUTPUT)/upgrade-recovery.fw" "$(OUTPUT)/upgrade-recovery.fw.sha256"
