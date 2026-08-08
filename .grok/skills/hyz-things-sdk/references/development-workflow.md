# Cross-repository development workflow

## Branch model

Use three layers:

| Layer | Example | Purpose |
|---|---|---|
| Immutable baseline | `sdk-main` | Exact imported vendor snapshot |
| Product integration | `hyz-things/main` | Latest coherent product state |
| Task branch | `hyz/flutter-shell` | One reviewable feature or fix |

Only create `hyz-things/main` in repositories that diverge from baseline. Unmodified components continue using `sdk-main`.

## Starting a task

1. Save a pinned pre-task manifest.
2. Run `repo status` and per-project `git status`.
3. List affected repositories and dependency order.
4. Fetch the relevant remotes without changing working trees.
5. Create the same task branch name only in affected repositories.

Typical command:

```sh
cd ~/Code/hyz_things/sdk
repo start hyz/<topic> buildroot device/rockchip kernel
```

Do not start branches in all 19 repositories merely for consistency.

## Commit structure

Example router/toolchain feature:

- `linux-buildroot`: internal glibc toolchain and product rootfs configuration.
- `rk-kernel`: router-related kernel fragment.
- `linux-device-rockchip`: toolchain bootstrap and board selection.
- `manifests`: product branch composition and pinned integration revision.

Each repository gets its own commit with a complete explanation. Do not create a single patch archive as the primary source of truth.

## Manifest strategy

The manifests repository should eventually contain:

```text
default.xml                     # Existing vendor baseline tracker
atk-linux-5.10-v1.2.xml         # Existing pinned vendor baseline
hyz-things.xml                  # Product development branches
releases/hyz-things-vX.Y.xml    # Exact release commits
```

In `hyz-things.xml`:

- Changed repositories use `revision="hyz-things/main"`.
- Unchanged repositories use `revision="sdk-main"`.
- Product applications are explicit projects.
- Paths remain stable once published.

Before publishing a pinned release manifest, sync it into an empty directory and verify commit IDs, linkfiles, LFS objects, and expected project count. Building is a separate, explicitly authorized verification step.

## Sync safety

Before `repo sync`:

```sh
repo status
repo manifest -r -o .repo/pre-sync.xml
```

Never use destructive cleanup to solve sync failures without identifying the owning repository and preserving local work. Do not use `git reset --hard`, `git clean`, force-push, or branch deletion unless the user explicitly approves the exact operation.

## Current linkfile exception

The baseline manifest creates:

```text
sdk/kernel/make.sh -> kernel_build.sh
```

It appears as an untracked file inside `rk-kernel`. It is expected only when it is exactly that symlink. Do not commit it accidentally. Any other dirty or untracked path requires investigation.

## Builds

No build runs automatically. When requested, use staged verification:

1. Configuration/static checks.
2. Buildroot toolchain only.
3. Kernel and external module.
4. Rootfs.
5. U-Boot/loader.
6. Full image or deployment artifact, if the required packing tools exist.

Record commands, logs, duration, outputs, and whether hardware testing occurred.
