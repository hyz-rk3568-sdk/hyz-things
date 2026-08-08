# Flutter Hello World

A minimal Flutter application for the RK3568 Buildroot target. It uses Sony's `flutter-elinux` embedder with the Arm64 Wayland backend and runs as a Weston client.

From the workspace root:

```sh
./scripts/build-flutter-hello.sh
ADB_SERIAL=a85f49b2a92e37ac ./scripts/deploy-flutter-hello-adb.sh
```

The release bundle is generated at:

```text
apps/flutter/hello_world/build/elinux/arm64/release/bundle/
```

The deployment script validates that the ADB target is AArch64 Buildroot, copies the bundle to `/tmp/hyz_things/flutter_hello`, and launches an `800x480` window on `/run/wayland-0`.

Stop the deployed application with:

```sh
adb -s a85f49b2a92e37ac shell 'kill $(cat /tmp/hyz_things/flutter_hello.pid)'
```
