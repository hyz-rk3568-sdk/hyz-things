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

The production startup service waits for `/run/wayland-0` and launches the application at the ATK 5.5-inch MIPI panel's native `1080x1920` resolution.

Stop the deployed application with:

```sh
adb -s a85f49b2a92e37ac shell 'kill $(cat /tmp/hyz_things/flutter_hello.pid)'
```
