---
name: hyz-things-sdk
description: Plan and perform development in the hyz_things RK3568 Android-repo workspace. Use when modifying Buildroot, device/rockchip, Linux kernel, U-Boot, rkbin, Wi-Fi/media components, Rust or Flutter integration, manifests, cross-repository branches, builds, or releases under ~/Code/hyz_things.
when-to-use: Use for requests such as 修改SDK, 改Buildroot, 改内核, 添加hyz_things配置, 集成Rust或Flutter, repo分支管理, SDK编译, 固件发布, or /hyz-things-sdk.
argument-hint: SDK task or component
user-invocable: true
compatibility: Requires Android repo, git, gh, and the 20-project hyz-rk3568-sdk workspace.
metadata:
  author: hyz
  short-description: Safely develop the hyz_things RK3568 SDK
---

# hyz_things SDK development

Treat this workspace as a product repository wrapped around an Android `repo` checkout. Preserve the imported vendor baseline and make every product change traceable to the component repository that owns it.

## Locate the workspace

- Workspace root: the directory containing `.grok/skills/hyz-things-sdk/`.
- SDK root: `<workspace>/sdk`.
- Product-owned applications: `<workspace>/apps`.
- Design and operational documentation: `<workspace>/docs`.
- Build outputs: `<workspace>/output`; never commit them.
- Do not assume the session started in either root. Resolve paths before editing.

Read [references/repository-map.md](references/repository-map.md) before selecting a repository. Read [references/development-workflow.md](references/development-workflow.md) before creating branches, commits, or manifests.

## Non-negotiable rules

1. **Keep `sdk-main` immutable.** It is the deterministic vendor baseline. Never commit product changes directly to it.
2. **Use `hyz-things/main` for integrated product state.** Start task branches from that branch once it exists; during initial bootstrap, create it from `sdk-main` only in repositories that need changes.
3. **Touch only owning repositories.** Do not copy SDK source into `platform/` as a substitute for a proper component commit.
4. **Keep commits repository-local and cohesive.** A kernel change, Buildroot change, and device-script change are separate commits even when they implement one feature.
5. **Use a manifest as the cross-repository lock.** A feature is not reproducible until the exact component commits can be represented by a pinned manifest.
6. **Do not build unless the user explicitly requests it.** Configuration parsing, shell syntax checks, XML validation, and Git inspection are allowed; a kernel, Buildroot, U-Boot, toolchain, rootfs, image, Rust, or Flutter build requires explicit permission.
7. **Do not push, open PRs, publish releases, or alter GitHub state without current user authorization.** Never force-push product or baseline branches.
8. **Keep `tools` minimal and do not restore `prebuilts` implicitly.** `tools` intentionally contains only `linux/Linux_Pack_Firmware`; report any request for other vendor tools or prebuilt GCC before adding them.
9. **Do not commit generated files or secrets.** This includes `output/`, Buildroot output trees, Rust `target/`, Flutter `build/`, Wi-Fi credentials, signing keys, and local `.config` files unless a reviewed defconfig is the intended source artifact.
10. **Preserve user work.** Before sync, checkout, branch switching, cleaning, or resetting, inspect all touched repositories. Never discard unexpected changes.

## Task procedure

### 1. Establish state

From `<workspace>/sdk`, inspect:

```sh
repo status
git -C <project-path> status --short --branch
repo manifest -r -o .repo/before-task.xml
```

The manifest currently creates `kernel/make.sh` as a linkfile, so it may appear as the single untracked kernel path. Treat only that exact symlink (`make.sh -> kernel_build.sh`) as expected; investigate every other change.

Check that the requested source repository is among the 20 manifest projects. If a needed dependency is absent, stop and explain whether it should become a new private component repository and manifest project.

### 2. Classify the change

- Buildroot package, rootfs overlay, toolchain, Weston/Qt, router userspace: `sdk/buildroot`.
- Board selection, build order, image orchestration, Wi-Fi post-processing: `sdk/device/rockchip`.
- DTS, kernel fragment, netfilter, drivers: `sdk/kernel`.
- Boot flow: `sdk/u-boot`; DDR/BL31 binary data: `sdk/rkbin` only when unavoidable.
- Vendor Wi-Fi/media library source: its corresponding `sdk/external/*` repository.
- Product Rust or Flutter source: `apps/*`, preferably as separate repositories added to the manifest later.

State the repositories to be touched before editing. Minimize that set.

### 3. Create task branches

Use one topic name across only the affected projects, for example `hyz/flutter-shell` or `hyz/router-foundation`:

```sh
cd <workspace>/sdk
repo start hyz/<topic> <project-path> [<project-path> ...]
```

Do not use `repo start ... --all`. Confirm each branch base. If `hyz-things/main` exists remotely, base the topic on it rather than `sdk-main`.

### 4. Implement in dependency order

For cross-repository work, normally proceed:

1. Application or external library source.
2. Buildroot package/configuration.
3. Kernel DTS/config/driver.
4. Device build scripts and board defconfig.
5. U-Boot/rkbin only if required.
6. Manifest integration last.

Use the product name `hyz_things` in new board/profile identifiers. Do not revive `router_media` naming.

### 5. Verify proportionally

Always perform read-only/static checks first:

- Review `git diff --check` and `git diff` in every touched repository.
- Validate shell with `bash -n` or `sh -n` as appropriate.
- Parse XML and verify manifest project/path uniqueness.
- Validate Kconfig symbols against the version in this SDK.
- For Buildroot, distinguish defconfig generation from compilation and tell the user before either.

Only run builds explicitly authorized by the user. Start with the smallest target, preserve logs outside source repositories, and report exact commands and output paths.

### OTA deployment transport policy

For routine recovery-free OTA installation, use this transport order:

1. Prefer USB wired ADB. Large firmware may be transferred with `adb push`, but verify its SHA-256 again on the board before staging the BCB.
2. Use network ADB only when wired ADB is unavailable. Do not send the large firmware with network `adb push`; bind a temporary HTTP server to the host's selected LAN address and let `hyz-router ota download` stream and verify it.

Always audit that the normal OTA excludes recovery and userdata, stop the temporary HTTP server immediately after a verified download, expect ADB to disconnect during recovery/reboot, and wait for strict router/proxy/system readiness rather than treating ADB availability as boot success. Follow the full commands and interrupted-transfer handling in [the OTA deployment guide](../../../docs/ota-deployment.md). Network ADB connection behavior and the WSL host standalone-server caveat are documented in [docs/network-adb.md](../../../docs/network-adb.md). Never place transient device addresses, serials, ports, or credentials in this skill.

### 6. Commit and integrate

- Commit separately in each component repository.
- Record `<repository, branch, commit, verification>` in the task summary.
- Merge reviewed topic branches into `hyz-things/main` without rewriting `sdk-main`.
- Update a development manifest to point changed projects at `hyz-things/main` while unchanged projects remain on `sdk-main`.
- Produce a pinned release manifest containing exact commits after integration verification.

### 7. Final report

Report:

- Component repositories changed.
- Commit IDs or uncommitted status.
- Static checks and builds actually run.
- Manifest/reproducibility status.
- Known limitations, especially omitted prebuilts, the intentionally minimal tools repository, and untested hardware behavior.
- Any external action still requiring approval.

## Adaptive maintenance

Treat this skill and its references as maintained project guidance, not as a fixed implementation recipe.

1. **Follow the user's current task scope.** The roadmap defines the preferred integration order, but an explicitly requested, isolated experiment may evaluate a later phase without automatically starting earlier phases.
2. **Trust inspected workspace state over stale guidance.** When the workspace differs from this skill, inspect the actual repositories, manifest, branches, toolchain, and build state before acting. Do not force the workspace to match outdated documentation.
3. **Make the smallest necessary correction.** Diagnose the owning layer before adding workarounds, copying files, changing SDK configuration, restoring omitted components, or expanding the repository set.
4. **Maintain durable project facts.** Update this skill or its references when development establishes a lasting change to repository ownership, manifest composition, branch policy, supported board/toolchain configuration, reproducible build entry points, selected runtime architecture/embedder, or long-term workflow.
5. **Keep task details in project documentation.** Put setup procedures and reusable troubleshooting in `<workspace>/docs`. Do not place transient logs, device serials, process IDs, local proxy values, generated output paths, or one-off failures in the skill.
6. **Do not weaken guardrails silently.** If a discovered requirement conflicts with a non-negotiable rule, stop and ask the user. Never silently relax baseline immutability, build authorization, external-action approval, or user-work preservation.
7. **Close the feedback loop.** At the end of a task, compare the result with the roadmap and references. Update only the sections made stale by durable findings; do not mark an integration phase complete from a prototype alone.

## Product roadmap

Follow [references/sdk-roadmap.md](references/sdk-roadmap.md). The immediate order is reproducible branch/manifest setup, Buildroot-generated toolchain, board profile `hyz_things`, router/media enablement, Rust services, then a measured Flutter embedder decision. Do not combine all stages into one unreviewable change.
