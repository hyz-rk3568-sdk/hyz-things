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

**Shared-component stage result: passed.** Plan section 7 is green. Overview redesign, theme work, and visual cleanup remain deferred until the capability-focused E2E organization below is green.

## Current stage — capability-focused Playwright suites

- Replace the ~80 KB `e2e/portal.spec.ts` monolith with capability suites for shell/overview, countdown, HTTP/security boundary, camera, proxy, network, and Tailscale.
- Move only genuinely shared browser helpers into `e2e/support.ts`; keep harness/network setup in `fixtures.ts`.
- Preserve every existing top-level Playwright test block and the per-test harness reset instead of weakening assertions or rewriting behavior during the move.
- The migration is required to be idempotent, reject duplicate test titles, reject empty capability suites, and verify the total test count against a generated marker before deleting the monolith.
- Full PR CI must still report the same 32 passing Chromium tests after the split before work proceeds to the Overview information-priority redesign.
