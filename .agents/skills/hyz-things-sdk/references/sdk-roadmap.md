# hyz_things SDK roadmap

Implement this roadmap as small cross-repository integrations. Do not start later integration phases until the previous phase has a reproducible manifest and documented verification.

The roadmap defines the preferred product integration order, not a prohibition on explicitly requested isolated prototypes. A prototype does not complete a phase until its source, build integration, manifest state, and documented verification are reproducible.

## Phase 0 — Product source control

- Keep all 19 `sdk-main` branches immutable.
- Create product integration branches only where changes are required.
- Add `hyz-things.xml` and a pinned product baseline manifest.
- Decide whether the workspace metadata (`.grok`, `docs`, scripts, deployment files) becomes a dedicated private repository.

## Phase 1 — Reproducible toolchain and build entry points

- Add a Buildroot internal AArch64 glibc toolchain profile.
- Make kernel, external modules, and U-Boot consume that generated compiler.
- Bootstrap the toolchain before kernel builds.
- Remove runtime assumptions about omitted top-level `prebuilts`.
- Keep `tools` limited to the required firmware packer and reject unreviewed vendor-tool growth.

## Phase 2 — `hyz_things` board profile

- Add a device defconfig named from `hyz_things`.
- Add matching Buildroot defconfig/fragment and kernel fragment.
- Preserve the ATK RK3568 DDR4 V1.0 DTS.
- Exclude Chromium, NPU, tests, and vendor demo/business applications unless explicitly restored.

## Phase 3 — Router and media foundation

- Wi-Fi uplink using RTL8852BS.
- Dual-GMAC LAN design, bridge/NAT/firewall, DHCP/DNS, and persistent interface naming.
- RKAIQ camera, MPP codec, RGA, GStreamer, ALSA, Mali, Weston/Qt as individually testable capabilities.
- Fix RK3568-specific RKADK/Rockit dependencies only after checking the shipped source actually supports the target APIs.

## Phase 4 — Rust services

- Target `aarch64-unknown-linux-gnu` with the same Buildroot sysroot.
- Keep Rust applications in dedicated repositories under `apps/rust`.
- Package them through Buildroot instead of copying binaries manually.
- Add service supervision, configuration schema, logging, and upgrade strategy.

## Phase 5 — Flutter evaluation and integration

Evaluate with a minimal GPU/input prototype before selecting an embedder:

- Sony `flutter-embedded-linux`: suitable for a Wayland/EGL-oriented product stack.
- `ardera/flutter-pi`: suitable for direct DRM/KMS without X11 or Wayland.

Measure:

- AArch64 engine compatibility and glibc requirements.
- Mali EGL/GLES and DRM/GBM behavior.
- Touch/keyboard input.
- Video texture and hardware-decoding integration.
- Startup time, memory, binary size, and coexistence with Weston/Qt.

Keep Flutter application source separate from engine/embedder packaging. Add the chosen application and embedder repositories to `hyz-things.xml` only after the prototype is reproducible.

## Phase 6 — Release and hardware validation

- Generate a pinned release manifest.
- Restore it in an empty directory.
- Build only with explicit authorization.
- Test boot, Ethernet, Wi-Fi uplink, NAT, camera, codec, audio, HDMI, GPU, Rust services, and Flutter UI on hardware.
- Record image hashes and component commits.
