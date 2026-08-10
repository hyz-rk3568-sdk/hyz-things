# hyz-things Development Guide

## User stories

@./docs/soft-router-user-stories.md

## Project architecture

@./docs/router.md

### Router implementation guardrails

The following rules apply to `apps/rust/router`:

- `src/domain` owns pure desired/observed state, actions, value objects, and invariants. It must not depend on Axum, Yew, Linux paths, processes, commands, or concrete adapters.
- `src/application` owns router, proxy, DHCP, panel, status, OTA, fail-open, and shutdown use cases. Application code may depend only on domain types and application-level ports.
- `src/adapters/inbound` is the driving-adapter layer. CLI, Unix control, udhcpc hook, and HTTP handlers must cross typed inbound boundaries and call application use cases instead of embedding business rules or concrete Linux execution details.
- `src/web` is a browser-side driving adapter. It communicates through the constrained HTTP API and must not gain a Rust dependency on native inbound or outbound adapters.
- `src/adapters/outbound` is the driven-adapter layer. It implements application ports and must not depend on HTTP DTOs, Yew components, or unvalidated caller-provided commands.
- `src/main.rs` is the only production composition root. Construction of `LinuxRouterPlatform`, `LinuxMihomoFailOpenPlatform`, `FirmwareAdapter`, and other concrete production adapters belongs there.
- Keep external execution fixed and typed: use fixed executable paths and typed argv; never introduce `sh -c` or pass browser-provided commands, paths, URLs, timeouts, provider names, or raw Mihomo JSON into platform execution.
- Preserve conservative ownership and readiness semantics. Unknown, foreign, stale, or partially observed resources must not be treated as owned or ready, and cleanup must remove only runtime-owned state.
- Preserve the management-plane safety boundary: router or proxy failures may degrade to a strictly confirmed management-only state, but must not expose HTTP readiness while network safety is unconfirmed.
- Do not expose router enable/disable, OTA, arbitrary configuration, or the Mihomo controller through the LAN API. Existing Web mutations must remain fixed, typed, same-origin, size-limited, and CSRF-protected.

## TDD

测试即文档。

@./docs/router.md

### Router test guardrails

- Tests should drive behavior through domain functions, application use cases, ports, `ControlHandler`, or HTTP/control seams instead of directly depending on concrete `apps/rust/router/src/adapters/outbound` implementations.
- When a test needs custom platform behavior, add or extend a stable application-level port or server-facing injection seam first; do not couple the test to `LinuxRouterPlatform` internals.
- Use fake ports to document desired/observed reconciliation, exact action order, rollback, ownership, fail-open, shutdown, OTA verify-before-commit, and unknown-state rejection.
- HTTP tests must continue to document the security boundary: exact routes and methods, typed JSON, body limits, origin and CSRF checks, secret exclusion, security headers, and SPA/API fallback behavior.
- Integration tests that execute real Linux commands or mutate network, block devices, process state, `/run`, or `/userdata` are target-device tests and must not run as ordinary host tests.
- Keep tests deterministic. Do not depend on live WAN access, real provider data, wall-clock timing, device credentials, generated firmware, or ignored audit artifacts.
- Run static checks before builds. Do not start Rust, frontend, SDK, Buildroot, kernel, rootfs, or firmware builds unless the user explicitly requests them.
