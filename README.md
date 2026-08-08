# hyz_things

Product workspace for the ATK RK3568 board. The product repository owns the
one-command build, Rust/Flutter applications, rootfs overlay, and release
contract. Rockchip vendor sources remain separate repositories pinned by an
Android `repo` manifest.

## Build from a clean clone

Host requirements: Linux x86-64, Git, curl, Python 3, GNU Make, Rustup,
Ninja, Clang/LLD, and the normal Buildroot host packages. The build creates its
own compatible CMake, flex, lz4, and dtc under the Buildroot host directory; a
system Python 2 installation is not required.

```sh
git clone https://github.com/hyz-rk3568-sdk/hyz-things.git
cd hyz-things
make
```

On the first run, `make` bootstraps and syncs the pinned SDK automatically.
Later builds reuse the existing SDK checkout; run `make sdk` explicitly when you
intend to synchronize it. For the exact component commits used by the verified
firmware build, run `make MANIFEST=hyz-things-release.xml`; normal development
uses `hyz-things.xml`.

`make upgrade` performs these stages in dependency order:

1. Selects `hyz_things_rk3568_defconfig`.
2. Builds the Buildroot AArch64 glibc toolchain.
3. Cross-builds the Rust Hello World and Rust OTA client.
4. Cross-builds the Sony eLinux Arm64/Wayland Flutter Hello World.
5. Stages the applications and startup service into the product rootfs overlay.
6. Builds Buildroot rootfs, kernel, and loader/boot images.
7. Packs a Rockchip update image as `output/upgrade.fw` and writes
   `output/upgrade.fw.sha256`.

The product profile reports itself to ADB as `product:hyz_things`,
`model:HYZ_RK3568`, and `device:rk3568`. It keeps one Simplified Chinese
regular font face and omits the kernel `System.map` from the runtime image. The OTA package
also deliberately excludes `userdata.img`, so an update does not reformat the
existing userdata partition; factory firmware generation still creates that
partition image.

The verified conservative profile produces a roughly 334 MiB update image
instead of the original 400 MiB image. Wi-Fi and Bluetooth remain in the
broad `ALL_AP` compatibility configuration until the exact production module
has been confirmed on hardware.

Useful smaller targets are listed by `make help`. `make check` runs unit,
format, shell, and Buildroot configuration checks without building firmware.

The Sony Flutter toolchain is pinned to `3.27.1`. Initial setup details and
proxy recovery notes remain in [`docs/flutter-elinux-setup.md`](docs/flutter-elinux-setup.md).

## Minimal OTA test

Serve the two release files from any HTTP server:

```sh
cd output
python3 -m http.server 8000
```

On the RK3568 target, download and verify with the Rust client:

```sh
SHA256=$(wget -qO- http://SERVER:8000/upgrade.fw.sha256 | awk '{print $1}')
hyz-ota download http://SERVER:8000/upgrade.fw "$SHA256"
```

Install without automatic reboot first:

```sh
hyz-ota install /userdata/upgrade.fw "$SHA256"
```

Or download, verify, install, and reboot in one command:

```sh
hyz-ota apply http://SERVER:8000/upgrade.fw "$SHA256" --reboot
```

`hyz-ota` uses `ureq` with Rustls for HTTP(S), `sha2` for streaming SHA-256,
checks the Rockchip `RKFW` header, and only then invokes Rockchip
`updateEngine`.

## Current safety boundary

This is a bring-up OTA chain, not the final production updater. SHA-256 protects
against transfer corruption but is not publisher authentication because an
attacker controlling the server could replace both files. Before field use,
add signed release metadata, anti-rollback state, A/B or recovery boot,
health-confirmed activation, and power-loss/fault-injection tests.
