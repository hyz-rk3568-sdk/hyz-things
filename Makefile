SHELL := /bin/bash
.DEFAULT_GOAL := upgrade

BOARD := hyz_things_rk3568
BR_BOARD := rockchip_hyz_things
BR_OUT := $(CURDIR)/sdk/buildroot/output/$(BR_BOARD)
BR_HOST := $(BR_OUT)/host
BR_SYSROOT := $(BR_HOST)/aarch64-buildroot-linux-gnu/sysroot
BR_PKG_CONFIG := $(BR_HOST)/bin/pkg-config
BR_PKG_CONFIG_LIBDIR := $(BR_SYSROOT)/usr/lib/pkgconfig:$(BR_SYSROOT)/usr/share/pkgconfig
TOOLCHAIN_PREFIX := $(BR_HOST)/bin/aarch64-buildroot-linux-gnu-
RUST_TARGET := aarch64-unknown-linux-gnu
ROUTER_APP := $(CURDIR)/apps/rust/router
CAMERA_APP := $(CURDIR)/apps/rust/camera
THINGS_APP := $(CURDIR)/apps/rust/things
CONTRACT_APP := $(CURDIR)/apps/rust/contract
THINGS_FRONTEND_BUNDLE := $(CURDIR)/target/frontend-bundle/hyz-things-frontend.tar
LOCAL_TRUNK_BIN := $(CURDIR)/.tools/trunk/bin
ROUTER_TRUNK := $(LOCAL_TRUNK_BIN)/trunk
ROUTER_BINARY := $(ROUTER_APP)/target/$(RUST_TARGET)/release/hyz-router
CAMERA_BINARY := $(CAMERA_APP)/target/$(RUST_TARGET)/release/hyz-camera
THINGS_BINARY := $(THINGS_APP)/target/$(RUST_TARGET)/release/hyz-things
OUTPUT := $(CURDIR)/output
OVERLAY := $(OUTPUT)/rootfs-overlay
REPO := $(CURDIR)/.tools/repo
MANIFEST_URL ?= https://github.com/hyz-rk3568-sdk/manifests.git
MANIFEST ?= hyz-things.xml
RECOVERY_PACKAGE := package-file-hyz-ota-with-recovery
ADB ?= adb
ADB_SERIAL ?=
JOBS ?= $(shell nproc)
HOST_NODE ?= $(shell command -v node 2>/dev/null)
HOST_NPM ?= $(shell command -v npm 2>/dev/null)
HOST_NODE_DIR := $(dir $(HOST_NODE))

# Buildroot rejects whitespace in PATH. Filter out WSL-injected Windows paths
# and whitespace entries, but keep the rest of the environment PATH so user
# tools (e.g. adb under platform-tools) stay usable inside make.
ENV_PATH := $(shell printf '%s\n' "$$PATH" | tr ':' '\n' | grep -v '^/mnt/' | grep -v ' ' | paste -sd: -)
BUILD_PATH := $(BR_HOST)/bin:$(LOCAL_TRUNK_BIN):$(ENV_PATH)
export PATH := $(BUILD_PATH)
export RK_TOOLCHAIN_PREFIX := $(TOOLCHAIN_PREFIX)

.PHONY: help sdk configure toolchain things-frontend things-e2e things-app router-app camera-app deploy-things deploy-camera revert-things revert-camera apps overlay rootfs kernel loader recovery firmware upgrade upgrade-recovery check check-static clean

help:
	@printf '%s\n' \
	  'make sdk        Clone/sync the pinned SDK manifest' \
	  'make toolchain  Configure Buildroot and build the AArch64 toolchain' \
	  'make things-frontend  Build the deterministic Yew/Tailwind frontend bundle' \
	  'make things-e2e  Run the host-only Axum/Playwright browser tests' \
	  'make things-app Build the hyz-things portal ELF with the embedded Yew UI' \
	  'make router-app Build the headless hyz-router core ELF' \
	  'make camera-app Build the independent hyz-camera ELF against the Buildroot sysroot' \
	  'make deploy-things  Build and hot-push hyz-things (router keeps running)' \
	  'make deploy-camera  Build and hot-push hyz-camera (router keeps running)' \
	  'make revert-things  Roll hyz-things back to its previously deployed binary' \
	  'make revert-camera  Roll hyz-camera back to its previously deployed binary' \
	  'make apps       Build the hyz-things, hyz-router and hyz-camera product applications' \
	  'make overlay    Stage product applications and metadata' \
	  'make rootfs     Stage product applications and build the Buildroot rootfs' \
	  'make kernel     Build the RK3568 kernel with the Buildroot compiler' \
	  'make recovery   Build the source-controlled recovery image' \
	  'make firmware   Build loader, kernel, recovery, rootfs, and partition images' \
	  'make upgrade    Build the normal OTA without recovery (default)' \
	  'make upgrade-recovery  Also build an explicit OTA containing recovery' \
	  'make check-static  Run source/configuration checks without compiling' \
	  'make check      Run formatting, tests, strict Clippy, and static checks'

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

things-frontend:
	test -x "$(ROUTER_TRUNK)" || { echo 'repository-local Trunk 0.21.14 is required under .tools/trunk.' >&2; exit 1; }
	test -x "$(HOST_NODE)" && test -x "$(HOST_NPM)" || { echo 'Node.js and npm must be available when make starts; override HOST_NODE/HOST_NPM if needed.' >&2; exit 1; }
	test -x "$(THINGS_APP)/node_modules/.bin/tailwindcss" || { echo 'run npm ci in apps/rust/things first.' >&2; exit 1; }
	rustup target add wasm32-unknown-unknown
	PATH="$(HOST_NODE_DIR):$(PATH)" bash "$(THINGS_APP)/tools/build-frontend-bundle.sh"
	test -s "$(THINGS_FRONTEND_BUNDLE)"

things-e2e:
	test -x "$(ROUTER_TRUNK)" || { echo 'repository-local Trunk 0.21.14 is required under .tools/trunk.' >&2; exit 1; }
	test -x "$(HOST_NODE)" && test -x "$(HOST_NPM)" || { echo 'Node.js and npm must be available when make starts; override HOST_NODE/HOST_NPM if needed.' >&2; exit 1; }
	cd "$(THINGS_APP)" && PATH="$(HOST_NODE_DIR):$(PATH)" "$(HOST_NPM)" run test:e2e

things-app: toolchain things-frontend
	rustup target add $(RUST_TARGET)
	HYZ_THINGS_FRONTEND_ARCHIVE="$(THINGS_FRONTEND_BUNDLE)" \
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$(TOOLCHAIN_PREFIX)gcc" \
	CC_aarch64_unknown_linux_gnu="$(TOOLCHAIN_PREFIX)gcc" \
	  cargo build --locked --release --target $(RUST_TARGET) \
	  --manifest-path "$(THINGS_APP)/Cargo.toml" \
	  --bin hyz-things --features native

router-app: toolchain
	rustup target add $(RUST_TARGET)
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$(TOOLCHAIN_PREFIX)gcc" \
	CC_aarch64_unknown_linux_gnu="$(TOOLCHAIN_PREFIX)gcc" \
	  cargo build --locked --release --target $(RUST_TARGET) \
	  --manifest-path "$(ROUTER_APP)/Cargo.toml" \
	  --bin hyz-router --features native

camera-app: toolchain
	$(MAKE) -C sdk/buildroot O="$(BR_OUT)" $(BR_BOARD)_defconfig
	@if [[ ! -f "$(BR_OUT)/target/usr/lib/gstreamer-1.0/libgstapp.so" || \
	       ! -f "$(BR_OUT)/target/usr/lib/gstreamer-1.0/libgstvideo4linux2.so" || \
	       ! -f "$(BR_OUT)/target/usr/lib/gstreamer-1.0/libgstvideoparsersbad.so" ]]; then \
		cd sdk && ./build.sh buildroot-make:gst1-plugins-base-dirclean:gst1-plugins-good-dirclean:gst1-plugins-bad-dirclean; \
	fi
	cd sdk && ./build.sh buildroot-make:gstreamer1:gst1-plugins-base:gst1-plugins-good:gst1-plugins-bad:rockchip-mpp:gstreamer1-rockchip:dejavu
	test -f "$(BR_OUT)/target/usr/lib/gstreamer-1.0/libgstapp.so"
	test -f "$(BR_OUT)/target/usr/lib/gstreamer-1.0/libgstvideo4linux2.so"
	test -f "$(BR_OUT)/target/usr/lib/gstreamer-1.0/libgstvideoparsersbad.so"
	test -x "$(BR_OUT)/target/usr/libexec/gstreamer-1.0/gst-plugin-scanner"
	test -x "$(BR_PKG_CONFIG)"
	rustup target add $(RUST_TARGET)
	PKG_CONFIG="$(BR_PKG_CONFIG)" \
	PKG_CONFIG_ALLOW_CROSS=1 \
	PKG_CONFIG_SYSROOT_DIR="$(BR_SYSROOT)" \
	PKG_CONFIG_LIBDIR="$(BR_PKG_CONFIG_LIBDIR)" \
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$(TOOLCHAIN_PREFIX)gcc" \
	AR_aarch64_unknown_linux_gnu="$(TOOLCHAIN_PREFIX)ar" \
	CC_aarch64_unknown_linux_gnu="$(TOOLCHAIN_PREFIX)gcc" \
	CXX_aarch64_unknown_linux_gnu="$(TOOLCHAIN_PREFIX)g++" \
	  cargo build --locked --release --target $(RUST_TARGET) \
	  --manifest-path "$(CAMERA_APP)/Cargo.toml" \
	  --bin hyz-camera

apps: router-app camera-app things-app

deploy-things: things-app
	ADB="$(ADB)" ADB_SERIAL="$(ADB_SERIAL)" bash "$(THINGS_APP)/tools/deploy-app.sh" deploy things "$(THINGS_BINARY)"

deploy-camera: camera-app
	ADB="$(ADB)" ADB_SERIAL="$(ADB_SERIAL)" bash "$(THINGS_APP)/tools/deploy-app.sh" deploy camera "$(CAMERA_BINARY)"

revert-things:
	ADB="$(ADB)" ADB_SERIAL="$(ADB_SERIAL)" bash "$(THINGS_APP)/tools/deploy-app.sh" revert things

revert-camera:
	ADB="$(ADB)" ADB_SERIAL="$(ADB_SERIAL)" bash "$(THINGS_APP)/tools/deploy-app.sh" revert camera

overlay: apps
	rm -rf "$(OVERLAY)"
	mkdir -p "$(OVERLAY)"
	cp -a product/rootfs-overlay/. "$(OVERLAY)/"
	install -D -m 0755 "$(ROUTER_BINARY)" "$(OVERLAY)/usr/bin/hyz-router"
	install -D -m 0755 "$(CAMERA_BINARY)" "$(OVERLAY)/usr/bin/hyz-camera"
	install -D -m 0755 "$(THINGS_BINARY)" "$(OVERLAY)/usr/bin/hyz-things"
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
	cargo fmt --manifest-path "$(CONTRACT_APP)/Cargo.toml" --all -- --check
	cargo test --locked --manifest-path "$(CONTRACT_APP)/Cargo.toml"
	cargo clippy --locked --manifest-path "$(CONTRACT_APP)/Cargo.toml" \
	  --all-targets -- -D warnings
	cargo fmt --manifest-path "$(THINGS_APP)/Cargo.toml" --all -- --check
	cargo test --locked --manifest-path "$(THINGS_APP)/Cargo.toml" --features native
	cargo clippy --locked --manifest-path "$(THINGS_APP)/Cargo.toml" \
	  --all-targets --features e2e -- -D warnings
	cargo fmt --manifest-path "$(ROUTER_APP)/Cargo.toml" --all -- --check
	cargo test --locked --manifest-path "$(ROUTER_APP)/Cargo.toml" --features native
	cargo clippy --locked --manifest-path "$(ROUTER_APP)/Cargo.toml" \
	  --all-targets --features native -- -D warnings
	bash "$(THINGS_APP)/tools/test-deploy-app.sh"

check-static:
	sh -n "$(THINGS_APP)/tools/build-frontend-bundle.sh"
	bash -n "$(THINGS_APP)/tools/deploy-app.sh"
	bash -n "$(THINGS_APP)/tools/test-deploy-app.sh"
	grep -q 'REMOTE_APPS_DIR=/userdata/hyz-things/apps' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q 'REMOTE_REGISTRY=.*registry\.json' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q 'REMOTE_RUN_DIR=/run/hyz-things/apps' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q '\[things\]=/etc/init\.d/S83hyz-things' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q '\[camera\]=/etc/init\.d/S82hyz-camera' "$(THINGS_APP)/tools/deploy-app.sh"
	! grep -qE 'S8[23]hyz-(things|camera)[[:space:]]+(stop|start|restart)' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q 'assert_router_untouched' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q 'ROUTER_READY_MARKER=/run/hyz-router/ready' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q 'PROTOCOL_VERSION' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q 'CONTROL_PROTOCOL_VERSION' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q "mv '\$$target_next' '\$$target' && sync" "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q 'protocol_versions' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q 'registry\.json' "$(THINGS_APP)/tools/deploy-app.sh"
	grep -q 'npm run build:css' "$(THINGS_APP)/tools/build-frontend-bundle.sh"
	grep -q 'data-bin="hyz-things-web"' "$(THINGS_APP)/frontend/index.html"
	grep -q '^hyz-contract = { path = "\.\./contract", default-features = false, optional = true }' "$(THINGS_APP)/Cargo.toml"
	! grep -q 'hmac\|pbkdf2\|url = ' "$(ROUTER_APP)/Cargo.toml"
	grep -q '"name": "hyz-things-web"' "$(THINGS_APP)/package.json" "$(THINGS_APP)/package-lock.json"
	grep -q 'apps/rust/things/frontend/router.css' .gitignore
	! grep -q 'HYZ Router\|HYZ 路由器' "$(THINGS_APP)/src/web" "$(THINGS_APP)/frontend/index.html" "$(THINGS_APP)/e2e"
	sh -n "$(THINGS_APP)/tools/start-e2e-server.sh"
	python3 -m json.tool "$(THINGS_APP)/package.json" >/dev/null
	python3 -m json.tool "$(THINGS_APP)/package-lock.json" >/dev/null
	test -x "$(HOST_NODE)"
	"$(HOST_NODE)" --check "$(THINGS_APP)/e2e/camera-hardware.mjs"
	! grep -qE '100\.[0-9]+\.[0-9]+\.[0-9]+|HYZ_ROUTER_ADMIN_PASSWORD=.*[^"$$]' "$(THINGS_APP)/e2e/camera-hardware.mjs"
	grep -q '"lockfileVersion"' "$(THINGS_APP)/package-lock.json"
	grep -q '@plugin "daisyui"' "$(THINGS_APP)/frontend/app.css"
	grep -q '@source "../src/web/\*\*/\*.rs"' "$(THINGS_APP)/frontend/app.css"
	python3 -c 'from pathlib import Path; p = Path("$(THINGS_APP)/tools/externalize-trunk-bootstrap.py"); compile(p.read_bytes(), str(p), "exec")'
	grep -q 'router-bootstrap\.js' "$(THINGS_APP)/tools/externalize-trunk-bootstrap.py"
	grep -q 'bootstrap_version' "$(THINGS_APP)/tools/externalize-trunk-bootstrap.py"
	grep -q 'path == "router-bootstrap.js"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q 'static_asset_path' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	! grep -R -q 'unsafe-inline' "$(THINGS_APP)/src" "$(THINGS_APP)/frontend" "$(THINGS_APP)/tools"
	grep -q "script-src 'self' 'wasm-unsafe-eval'" "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	! grep -q "script-src 'self' 'unsafe-eval'" "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q 'Ipv4Addr::new(192, 168, 8, 1)' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/control/display"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/control/proxy/delay"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/control/proxy/delays"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/control/proxy/lan-tun"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/control/proxy/local-system"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/tailscale/peers"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	! grep -q '\.route("/api/v1/control/proxy/mode"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/apps"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q '"/api/v1/camera/viewer-token"' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q 'authorize_camera_viewer' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q 'INSTALLED_APPS_REGISTRY_PATH: &str = "/userdata/hyz-things/apps/registry.json"' "$(THINGS_APP)/src/adapters/outbound/registry.rs"
	grep -q 'CERT_DIRECTORY: &str = "/userdata/hyz-things/tls"' "$(THINGS_APP)/src/adapters/inbound/http/tls.rs"
	grep -q 'PortalTls' "$(THINGS_APP)/src/adapters/inbound/http/tls.rs"
	grep -q 'struct TlsListener' "$(THINGS_APP)/src/adapters/inbound/http/tls.rs"
	grep -q '{}://{LAN_ADDRESS}:{port}' "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	grep -q 'axum::serve(TlsListener' "$(THINGS_APP)/src/main.rs"
	grep -q 'PORTAL_READY_MARKER: &str = "/run/hyz-things/ready"' "$(THINGS_APP)/src/main.rs"
	grep -q 'app-camera-tab' "$(THINGS_APP)/src/web/app.rs"
	grep -q '\[function_component(CameraLiveView)\]' "$(THINGS_APP)/src/web/pages/camera.rs"
	grep -q 'MIHOMO_CONTROLLER_ADDRESS: &str = "127.0.0.1:9090"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	grep -q 'TAILSCALED_EXECUTABLE: &str = "/usr/bin/tailscaled"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	grep -q 'TAILSCALE_EXECUTABLE: &str = "/usr/bin/tailscale"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	grep -q 'TAILSCALE_STATE_FILE: &str = "/userdata/hyz-router/tailscale/tailscaled.state"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	grep -q 'TAILSCALE_SOCKET: &str = "/run/hyz-tailscale/tailscaled.sock"' "$(ROUTER_APP)/src/adapters/outbound/paths.rs"
	test -x "$(CURDIR)/product/rootfs-overlay/usr/bin/hyz-tailscale"
	sh -n "$(CURDIR)/product/rootfs-overlay/usr/bin/hyz-tailscale"
	grep -Fq 'exec /usr/bin/tailscale --socket /run/hyz-tailscale/tailscaled.sock "$$@"' "$(CURDIR)/product/rootfs-overlay/usr/bin/hyz-tailscale"
	grep -q 'TAILSCALE_INTERFACE: &str = "tailscale0"' "$(ROUTER_APP)/src/domain/tailscale.rs"
	grep -q 'TAILSCALE_LAN_ROUTE: &str = "192.168.8.0/24"' "$(ROUTER_APP)/src/domain/tailscale.rs"
	grep -q 'TAILSCALE_UDP_PORT: u16 = 41_641' "$(ROUTER_APP)/src/domain/tailscale.rs"
	grep -q 'pub const TAILSCALE_MANAGEMENT_HTTP_PORT: u16 = 8080' "$(CONTRACT_APP)/src/tailscale.rs"
	! grep -qE 'pub (login_server|auth_key|subnet|advertise_routes|exit_node|argv|tag):' \
	  "$(ROUTER_APP)/src/adapters/inbound/control.rs" \
	  "$(THINGS_APP)/src/adapters/inbound/http/mod.rs"
	! grep -R -qE '127\.0\.0\.1:9090|controller\.secret' "$(THINGS_APP)/src/web" "$(THINGS_APP)/frontend"
	grep -q 'default-brightness-level = <0>' sdk/kernel/arch/arm64/boot/dts/rockchip/rk3568-atk-evb1-mipi-dsi-1080p.dts
	! grep -qE '&(dsi1|dsi1_panel|backlight1)[[:space:]]*\{[[:space:]]*status = "disabled"' sdk/kernel/arch/arm64/boot/dts/rockchip/rk3568-atk-evb1-mipi-dsi-1080p.dts
	! grep -R -E -q 'TcpListener::bind\([^)]*(UNSPECIFIED|\[0,[[:space:]]*0,[[:space:]]*0,[[:space:]]*0\])|Ipv4Addr::UNSPECIFIED|CorsLayer::permissive|/usr/sbin/hyz-mihomo' "$(ROUTER_APP)/src" "$(THINGS_APP)/src"
	! grep -R -q 'Command::new("sh")\|Command::new("bash")' "$(ROUTER_APP)/src" "$(THINGS_APP)/src"
	test -f "$(CAMERA_APP)/Cargo.lock"
	! grep -R -q 'std::process::Command\|Command::new\|sh -c\|gst_parse_launch' "$(CAMERA_APP)/src"
	grep -q 'CONTROL_OWNER_PATH: &str = "/run/hyz-camera/daemon.owner"' "$(CAMERA_APP)/src/adapters/inbound/unix_control.rs"
	grep -q 'UdpSocket::bind(SocketAddr::from((address, port)))' "$(CAMERA_APP)/src/adapters/outbound/webrtc.rs"
	! grep -q 'Ipv4Addr::UNSPECIFIED' "$(CAMERA_APP)/src/adapters/outbound/webrtc.rs"
	grep -q 'CAMERA_UDP_PORT_START: u16 = 40_000' "$(CAMERA_APP)/src/domain/session.rs"
	grep -q 'CAMERA_UDP_PORT_END: u16 = 40_015' "$(CAMERA_APP)/src/domain/session.rs"
	grep -q 'width: 1920' "$(CONTRACT_APP)/src/camera.rs"
	grep -q 'height: 1080' "$(CONTRACT_APP)/src/camera.rs"
	grep -q 'bitrate_bps: 2_500_000' "$(CONTRACT_APP)/src/camera.rs"
	grep -q 'pub const FIXED_CAPTURE_WIDTH: u16 = 3840' "$(CONTRACT_APP)/src/camera.rs"
	grep -q 'pub const FIXED_CAPTURE_HEIGHT: u16 = 2160' "$(CONTRACT_APP)/src/camera.rs"
	grep -q 'WATERMARK_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S"' "$(CAMERA_APP)/src/domain/watermark.rs"
	grep -q 'WATERMARK_FONT_FAMILY: &str = "DejaVu Sans"' "$(CAMERA_APP)/src/domain/watermark.rs"
	grep -q 'WATERMARK_FONT_SIZE: u32 = 20' "$(CAMERA_APP)/src/domain/watermark.rs"
	grep -q '"{} {}px"' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q 'source.set_property("device", FIXED_CAMERA_DEVICE)' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q 'make("clockoverlay", "timestamp-overlay")' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q 'make("videoflip", "orientation-flip")' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q '"clockoverlay"' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q '"videoflip"' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q 'WatermarkPosition::TopLeft' "$(CAMERA_APP)/src/domain/watermark.rs"
	grep -q 'FULL_RANGE_BT709_COLORIMETRY: &str = "1:3:5:1"' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q 'field("colorimetry", FULL_RANGE_BT709_COLORIMETRY)' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q 'set_property_from_str("level", h264_level(profile))' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q '"5.1"' "$(CAMERA_APP)/src/adapters/outbound/gstreamer.rs"
	grep -q 'MAX_VIEWERS: usize = 4' "$(CAMERA_APP)/src/application/lifecycle.rs"
	grep -q 'struct FrameHub' "$(CAMERA_APP)/src/domain/stream.rs"
	grep -q 'fn subscribe(&self) -> Arc<BoundedFrameQueue>' "$(CAMERA_APP)/src/application/ports.rs"
	grep -q 'sessions: Vec<ActiveSession>' "$(CAMERA_APP)/src/application/lifecycle.rs"
	grep -q 'GST_VIDEO_COLOR_RANGE_0_255' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
	grep -q 'MPP_FRAME_RANGE_JPEG' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
	grep -q 'mpp_enc_cfg_set_s32 (self->mpp_cfg, "prep:range", range)' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
	grep -q 'failed to set input color range' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
	grep -q 'self->prop_dirty = TRUE' sdk/external/gstreamer-rockchip/gst/rockchipmpp/gstmppenc.c
	grep -q 'network-config-sta-rollback-v1.json' "$(ROUTER_APP)/src/adapters/outbound/network_config.rs"
	grep -q 'let cleanup_succeeded = cleanup.is_ok()' "$(ROUTER_APP)/src/main.rs"
	test ! -d "$(ROUTER_APP)/adapter-linux"
	sh -n sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'TARGET_DIR/usr/bin/hyz-ota' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'TARGET_DIR/usr/sbin/hyz-router' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'TARGET_DIR/usr/sbin/hyz-camera' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'chmod 0755.*S82hyz-camera' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'TARGET_DIR/etc/init.d/S82tailscaled' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	grep -q 'TARGET_DIR/usr/share/metacubexd' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	sh -n sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	grep -q '^DAEMON=/usr/bin/hyz-camera$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	grep -q '^CONTROL_SOCKET=\$$RUNTIME_DIR/control.sock$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	grep -q '^OWNER_FILE=\$$RUNTIME_DIR/daemon.owner$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	grep -q "cset name='Playback Path' SPK" sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	grep -q "cset name='Capture MIC Path' 'Main Mic'" sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	grep -q '^START_TIMEOUT_SECONDS=30$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	grep -q '^STOP_TIMEOUT_SECONDS=30$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	grep -q 'stale ownership requires explicit recovery' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	grep -q 'control readiness timeout; daemon left running' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	! grep -q 'hyz-router\|HEALTH_URL\|192\.168\.8\.1' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-camera
	sh -n sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^DAEMON=/usr/bin/hyz-router$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^START_TIMEOUT_SECONDS=300$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^START_EXEC_GRACE_ATTEMPTS=10$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^START_RETRY_BACKOFF_MAX=30$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^STOP_CLEANUP_TIMEOUT_SECONDS=130$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^FLOCK=/usr/bin/flock$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^ACTION_LOCKFILE=/run/hyz-router-init.lock$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	! grep -q 'restart)' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'Usage: \$$0 {start|stop|status}' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q -- '-- daemon 9>&-' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'deadline=.*START_TIMEOUT_SECONDS' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'daemon left running' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'wait_for_runtime_cleanup' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '"$$START_STOP_DAEMON" -K -q -s TERM' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '"$$START_STOP_DAEMON" -K -q -s KILL' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	! grep -q -- '-R ' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'retrying after attempt' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	! grep -q 'START_LAUNCH_ATTEMPTS\|START_ATTEMPTS' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q 'stale ownership requires explicit recovery' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	grep -q '^READY_MARKER=/run/hyz-router/ready$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	! grep -q 'HEALTH_URL=http://192\.168\.8\.1:8080' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S81hyz-router
	sh -n sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S83hyz-things
	grep -q '^DAEMON=/usr/bin/hyz-things$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S83hyz-things
	grep -q '^ROUTER_READY_MARKER=/run/hyz-router/ready$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S83hyz-things
	grep -q '^READY_MARKER=/run/hyz-things/ready$$' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S83hyz-things
	! grep -q 'HEALTH_URL\|wget' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S83hyz-things
	! grep -q 'restart)' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S83hyz-things
	grep -q 'chmod 0755 .*S83hyz-things' sdk/buildroot/board/rockchip/hyz_things/post-build.sh
	! grep -qE 'hyz-mihomo (explicit|tun|disable)|hyz-mihomo removes' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/hyz-router/mihomo-config.yaml.example
	grep -q 'hyz-router proxy lan-tun enable|disable' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/hyz-router/mihomo-config.yaml.example
	grep -q 'hyz-router proxy local-system enable|disable' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/hyz-router/mihomo-config.yaml.example
	! grep -qE '^(mixed-port|port|socks-port|redir-port|tproxy-port|allow-lan|bind-address|external-controller|secret|tun):' sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/hyz-router/mihomo-config.yaml.example
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/etc/init.d/S82hyz-mihomo
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/usr/sbin/hyz-router
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/usr/sbin/hyz-camera
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/usr/sbin/hyz-mihomo
	test ! -e sdk/buildroot/board/rockchip/hyz_things/fs-overlay/usr/share/udhcpc/default.script.d/50-hyz-wlan-metric
	grep -q 'BR2_ROOTFS_OVERLAY+="board/rockchip/hyz_things/fs-overlay ../../output/rootfs-overlay"' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_CAMERA_ENGINE=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_CAMERA_ENGINE_RKAIQ=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_CAMERA_ENGINE_RKAIQ_IQFILE="isp21/imx415_CMK-OT1522-FG3_CS-P1150-IRC-8M-FAU.json"$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q 'BUILD_RKAIQ_3A_ONLY' sdk/external/camera_engine_rkaiq/CMakeLists.txt
	grep -q -- '-DBUILD_RKAIQ_3A_ONLY=ON' sdk/buildroot/package/rockchip/camera-engine-rkaiq/camera-engine-rkaiq.mk
	grep -q '^BR2_PACKAGE_GSTREAMER1=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_GST1_PLUGINS_BASE=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_GST1_PLUGINS_BASE_PLUGIN_APP=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_GST1_PLUGINS_BASE_PLUGIN_PANGO=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_DEJAVU=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_DEJAVU_SANS=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_GST1_PLUGINS_GOOD=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_GST1_PLUGINS_GOOD_PLUGIN_V4L2=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_GST1_PLUGINS_GOOD_PLUGIN_VIDEOFILTER=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_GST1_PLUGINS_BAD=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_GST1_PLUGINS_BAD_PLUGIN_VIDEOPARSERS=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_ROCKCHIP_MPP=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_ROCKCHIP_MPP_ALLOCATOR_DRM=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	grep -q '^BR2_PACKAGE_GSTREAMER1_ROCKCHIP=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	@for symbol in GSTREAMER1_INSTALL_TOOLS GST1_PLUGINS_BASE_INSTALL_TOOLS \
	  GST1_PLUGINS_BASE_PLUGIN_PLAYBACK GST1_PLUGINS_BASE_PLUGIN_TYPEFIND \
	  GST1_PLUGINS_BASE_PLUGIN_VOLUME \
	  GST1_PLUGINS_GOOD_PLUGIN_AVI GST1_PLUGINS_GOOD_PLUGIN_ISOMP4 \
	  GST1_PLUGINS_GOOD_PLUGIN_RTP GST1_PLUGINS_GOOD_PLUGIN_RTPMANAGER \
	  GST1_PLUGINS_GOOD_PLUGIN_UDP GST1_PLUGINS_GOOD_PLUGIN_V4L2_PROBE \
	  GST1_PLUGINS_GOOD_PLUGIN_WAVPARSE GST1_PLUGINS_BAD_PLUGIN_DIRECTFB \
	  GST1_PLUGINS_BAD_PLUGIN_DTLS GST1_PLUGINS_BAD_PLUGIN_GL \
	  GST1_PLUGINS_BAD_PLUGIN_SRTP GST1_PLUGINS_BAD_PLUGIN_WAYLAND \
	  GST1_PLUGINS_BAD_PLUGIN_WPE LIBV4L ROCKCHIP_MPP_TESTS; do \
		grep -q "^# BR2_PACKAGE_$$symbol is not set$$" sdk/buildroot/configs/rockchip/hyz_things.config || exit 1; \
	done
	@for symbol in GST1_PLUGINS_BASE_PLUGIN_ALSA GST1_PLUGINS_BASE_PLUGIN_AUDIOCONVERT \
	  GST1_PLUGINS_BASE_PLUGIN_AUDIORESAMPLE GST1_PLUGINS_BASE_PLUGIN_AUDIOMIXER \
	  GST1_PLUGINS_BASE_PLUGIN_OPUS GST1_PLUGINS_BAD_PLUGIN_WEBRTCDSP \
	  ALSA_UTILS ALSA_UTILS_APLAY ALSA_UTILS_AMIXER ROCKCHIP_ALSA_CONFIG; do \
		grep -q "^BR2_PACKAGE_$$symbol=y$$" sdk/buildroot/configs/rockchip/hyz_things.config || exit 1; \
	done
	! grep -q '^BR2_PACKAGE_FFMPEG=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	! grep -q '^BR2_PACKAGE_LIBNICE=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	! grep -q '^BR2_PACKAGE_GST1_PLUGINS_BAD_PLUGIN_WEBRTC=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
	! grep -qiE '^BR2_PACKAGE_.*WEBRTCSINK=y$$' sdk/buildroot/configs/rockchip/hyz_things.config
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
