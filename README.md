# hyz_things

Product workspace for the ATK RK3568 board. The product repository owns the
one-command build, the unified Rust router application, rootfs overlay, and release
contract. Rockchip vendor sources remain separate repositories pinned by an
Android `repo` manifest.

## Build from a clean clone

Host requirements: Linux x86-64, Git, curl, Python 3, GNU Make, Rustup,
and the normal Buildroot host packages. The unified router frontend uses
repository-local Trunk 0.21.14 under `.tools/trunk`. The build creates its own compatible CMake, flex, lz4, and dtc under the
Buildroot host directory; a system Python 2 installation is not required.

```sh
git clone https://github.com/hyz-rk3568-sdk/hyz-things.git
cd hyz-things
make
```

On the first run, `make` bootstraps and syncs the development SDK manifest
automatically. Later builds reuse the existing SDK checkout; run `make sdk`
explicitly when you intend to synchronize it. For the exact component commits
used by a verified firmware build, run
`make MANIFEST=hyz-things-release.xml`; normal development uses
`hyz-things.xml`.

`make upgrade` performs these stages in dependency order:

1. Selects `hyz_things_rk3568_defconfig`.
2. Builds the Buildroot AArch64 glibc toolchain.
3. Builds the deterministic Yew bundle and cross-builds the unified `hyz-router` ELF.
4. Stages only `/usr/bin/hyz-router` and product metadata into the product overlay.
5. Builds Buildroot rootfs, kernel, loader/boot, and the source-controlled
   `output/recovery.img` with its SHA-256 file.
6. Packs the normal Rockchip OTA without `recovery.img` as
   `output/upgrade.fw` and writes `output/upgrade.fw.sha256`.

The control plane is consolidated as one hexagonal Rust package at
`apps/rust/router`, with one native `hyz-router` composition root for the
root-only daemon/control socket, LAN-only Axum/Yew status, typed display/proxy
controls, administrator-protected AP/STA and subscription settings, OTA, DHCP,
router, and Mihomo/TUN lifecycle. Wi-Fi changes use committed/pending generations
and rollback; subscription refresh accepts only constrained HTTPS/public targets,
sanitizes strict provider YAML, validates a Mihomo candidate, and cuts over last.
The same ELF also provides the internal udhcpc and detached fail-open watcher
roles. `make apps` builds this single product application, and `make overlay`
stages it only as `/usr/bin/hyz-router`; the Buildroot board overlay supplies the
minimal `S81hyz-router` init script. The legacy shell router/Mihomo wrappers,
separate DHCP hook, standalone OTA app, and Rust/Flutter hello demos have been
removed from active source staging. The previously unified runtime and embedded
Web UI were installed through recovery-free OTA and functionally validated on
RK3568. The new administrator/settings implementation and the capped init retry
backoff are source-only: neither has been compiled, installed, or hardware-tested.
Automatic cold-boot and settings acceptance therefore remain open.

The product profile reports itself to ADB as `product:hyz_things`,
`model:HYZ_RK3568`, and `device:rk3568`. It keeps one Simplified Chinese
regular font face and omits the kernel `System.map` from the runtime image. The OTA package
also deliberately excludes `userdata.img`, so an update does not reformat the
existing userdata partition; factory firmware generation still creates that
partition image.

The verified conservative base profile produced a roughly 334 MiB update image
instead of the original 400 MiB image. The latest installed unified Router/Web
validation image is 378,495,562 bytes; it remains a validation artifact rather
than a release because the source-only cold-boot backoff correction has not yet
been rebuilt and installed. MetaCubeXD has been retired in favor of the Yew UI
embedded in `hyz-router`. Wi-Fi and Bluetooth remain in the broad `ALL_AP`
compatibility configuration until the exact production module has been confirmed
on hardware.

### Recovery OTA policy

Recovery is built from the manifest-controlled `external/recovery` component
on every full firmware build, but the normal `output/upgrade.fw` deliberately
does not update it. This avoids rewriting the recovery environment for routine
application, rootfs, or kernel releases.

When recovery itself or its firmware compatibility changes, build both OTA
variants explicitly:

```sh
make upgrade-recovery
```

This first produces the normal `output/upgrade.fw`, then creates
`output/upgrade-recovery.fw` and its SHA-256 file from a separate package list
that includes `recovery.img`. Both OTA variants exclude `userdata.img`.
Publish the recovery variant only as an intentional infrastructure update;
do not substitute it for routine OTA releases.

Useful smaller targets are listed by `make help`. `make check-static` runs
shell, Python, source-reference, package, kernel-symbol, and manifest checks
without compiling or generating a Buildroot defconfig. `make check` additionally
runs router formatting, native tests, and strict Clippy, but still does not build
firmware.

The retired Sony Flutter prototype and its setup history remain documented in
[`docs/flutter-elinux-setup.md`](docs/flutter-elinux-setup.md), but they are no
longer part of the product application graph. The hardware profile still uses
the ATK 5.5-inch `1080x1920` MIPI-DSI panel target
`rk3568-atk-evb1-mipi-dsi-1080p`.
The panel's accepted low-gray brightness boundary is recorded as a stopped
known issue in
[`docs/display-low-gray-known-issue.md`](docs/display-low-gray-known-issue.md).
The approved soft-router scope, wired-first WAN policy, and acceptance criteria
are defined in
[`docs/soft-router-user-stories.md`](docs/soft-router-user-stories.md). The Wi-Fi-only
`br-lan = p2p0` source migration, kernel prerequisites, and pending board test plan
are recorded in
[`docs/soft-router-br-lan-validation.md`](docs/soft-router-br-lan-validation.md).
The pinned Mihomo input, historical proxy-dashboard validation, firmware hashes,
rootfs audit, and remaining board tests are recorded in
[`docs/soft-router-proxy-integration.md`](docs/soft-router-proxy-integration.md).
The source-level, LAN-only Mihomo TUN policy, persistent mode controls, and
fail-open boundaries are documented in
[`docs/soft-router-tun.md`](docs/soft-router-tun.md), including the final
recovery-free OTA hashes and board-validated TCP/UDP/fail-open results. The
LAN-only Axum/Yew status and constrained display/proxy controls, API boundary,
TDD coverage and remaining validation work are documented in
[`docs/router.md`](docs/router.md).

## Minimal OTA test

For the preferred USB ADB path, the network ADB fallback, interrupted-transfer handling, package audit, and post-boot readiness checks, follow [`docs/ota-deployment.md`](docs/ota-deployment.md). The minimal HTTP flow is:

Serve the two release files from any HTTP server:

```sh
cd output
python3 -m http.server 8000
```

On the RK3568 target, download and verify with the Rust client:

```sh
SHA256=$(wget -qO- http://SERVER:8000/upgrade.fw.sha256 | awk '{print $1}')
hyz-router ota download http://SERVER:8000/upgrade.fw "$SHA256"
```

Install without automatic reboot first:

```sh
hyz-router ota install /userdata/hyz-router/ota/upgrade.fw "$SHA256"
```

Or download, verify, install, and reboot in one command:

```sh
hyz-router ota apply http://SERVER:8000/upgrade.fw "$SHA256" --reboot
```

`hyz-router ota` uses `ureq` with Rustls for HTTP(S), `sha2` for streaming SHA-256,
and validates the Rockchip `RKFW` header. Routine recovery-free packages are
staged by writing and reading back Rockchip's 1088-byte bootloader control
block at the 16 KiB offset of `misc`; recovery then flashes the fixed
non-A/B partition mask. The explicit recovery image path uses a separate
command so updating recovery is never accidental:

```sh
hyz-router ota install-recovery /userdata/upgrade-recovery.fw "$SHA256"
```

That command invokes Rockchip `updateEngine` to install recovery first and
rejects its misleading zero exit status unless the resulting BCB exactly
matches the requested package.

## Current safety boundary

This is a bring-up OTA chain, not the final production updater. SHA-256 protects
against transfer corruption but is not publisher authentication because an
attacker controlling the server could replace both files. Before field use,
add signed release metadata, anti-rollback state, A/B support or another
rollback strategy, health-confirmed activation, and power-loss/fault-injection
tests.
