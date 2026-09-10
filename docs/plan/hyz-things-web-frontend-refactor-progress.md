# hyz-things Web frontend refactor progress

This file records implementation evidence while `hyz-things-web-frontend-refactor-plan.md` is being executed. The final Host matrix must still be copied back into the plan before the refactor is marked complete.

## 2026-09-10 — structural checkpoint

- Remaining page ownership has been extracted into `pages/overview.rs` and `pages/system.rs`.
- `pages/mod.rs` and `hooks/mod.rs` now provide the normal module roots instead of root-level `#[path]` aliases.
- `app.rs` owns portal composition/navigation/reducer orchestration; the System arm delegates page rendering to `pages/system.rs`.
- The structural driver passed migration, rustfmt, WASM `cargo check`, `git diff --check`, and generated-commit gates. The checkpoint script also guards against the E2E navigation migration becoming non-idempotent and against countdown unit-test registration being dropped.
- The PiP countdown Playwright helper now converts day/hour/minute/second fields into real seconds instead of adding display fields directly.
- The administrator STA/AP/subscription journey now has one canonical Network → Proxy → Network transition; the migration source no longer inserts duplicate STA assertions on repeated runs.
- Deterministic countdown coverage is registered under `hooks/countdown_tests.rs`. Full CI run `34478106507` executed the browser-side unit target and reported 9/9 passing tests. The five plan-specific cases all passed: future target, past target, target boundary, custom countdown/storage round-trip, and invalid storage data.
- On the same human-authored head `a894de845bf6a1c23a4fc1088cfac58e6a7152f4`, run `34478106507` passed `git diff --check`, `make check-static`, contract format/tests/Clippy, deterministic frontend bundle, things format, web unit logic, native tests (41 unit + 3 HTTP integration), strict Clippy, and artifact verification.
- The Playwright job in run `34478106507` passed all 32 tests in Chromium with no business assertions deleted.

**Structural stage result: passed.** Plan section 14.1 is satisfied for the current checkpoint.

## 2026-09-10 — shared UI component checkpoint

- Shared presentation ownership now covers `LoadingState`, `EmptyState`, `ErrorState`, `MetricCard`, `StatusBadge`, `PageHeader`, `SectionCard`, action feedback, `CountdownCard`, `CountdownEditor`, and `AppShell`.
- Countdown card/editor markup moved out of the countdown hook while timer state, persistence, pause/resume behavior, and Document/video PiP lifecycle remain owned by `hooks/countdown.rs`.
- `AppShell` owns portal chrome only. Polling, authentication, capability state, page lifecycle, and navigation decisions remain in the application layer.
- Component migration guards reject capability/API state such as `AppState`, `UseReducerHandle`, status DTOs, and request objects from shared presentation components.
- Human-authored CI run `34483983476` passed the committed diff/static checks, contract format/tests/Clippy, deterministic frontend bundle, Rust formatting, browser-side web unit logic, native tests, and strict Clippy.
- The same run's Playwright job executed `Running 32 tests using 1 worker` and completed with `32 passed (2.5m)`.

**Shared-component stage result: passed.** Plan section 7 is green.

## 2026-09-10 — capability-focused Playwright checkpoint

- The former ~80 KB `e2e/portal.spec.ts` monolith is gone. Playwright coverage is organized into `shell.spec.ts`, `countdown.spec.ts`, `security.spec.ts`, `camera.spec.ts`, `proxy.spec.ts`, `network.spec.ts`, and `tailscale.spec.ts`.
- Shared browser-only helpers live in `e2e/support.ts`; harness/network setup remains in `fixtures.ts`.
- The generated split preserves every former top-level test block and each suite resets the deterministic harness in `test.beforeEach`.
- `tools/check-e2e-suites.py` is a durable CI gate: it rejects a returning monolith, missing/extra capability suites, empty suites, missing harness reset, duplicate titles, declared-test drift, and runnable-test drift.
- There are 33 declared top-level tests. `playwright.config.ts` intentionally excludes exactly one real-CDP touch-input case with `grepInvert`, leaving a runnable CI baseline of 32 tests. The static gate records both values rather than confusing declared and runnable counts.
- Human-authored head `09403dc7ecbc616cbaf02fb8d4f0dece815e7488`, CI run `34487713839`, passed the suite-layout gate, `git diff --check`, `make check-static`, contract format/tests/Clippy, deterministic frontend build, things format, browser-side unit tests, native tests, and strict Clippy.
- The same run's Playwright job reported `Running 32 tests using 1 worker` and `32 passed (2.5m)` after the capability split.

**Capability-focused Playwright stage result: passed.** The refactor can now proceed to the Overview information-priority work without weakening the E2E baseline.

## Current stage — Overview information priority

- Put Internet/WAN, LAN/Wi-Fi, Proxy, and Tailscale health in the first visual layer, with explicit text status so color is supplementary rather than the only signal.
- Keep degraded/unknown/unavailable semantics honest; do not show an unconfirmed state as healthy.
- Move the existing network topology below the core health summary while retaining the real dual-uplink, Router/NAT, Proxy, and Tailscale data path.
- Keep issues prominent and preserve the most recent successful snapshot behavior.
- Keep Camera/Apps/countdown auxiliary to core device/network health; countdown stays on Overview and does not become first-level navigation.
- Preserve existing data sources and control behavior. Extend the capability-focused E2E assertions rather than adding selectors tied to Tailwind or DOM implementation detail.
- Full human-authored PR CI, including the 32-test runnable Playwright baseline, is required before this stage is marked green.
