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

## 2026-09-10 — Overview information-priority checkpoint

- The generated Overview implementation puts Internet/WAN, LAN/Wi-Fi, Proxy, and Tailscale in the first visual layer, with explicit `正常` / `需检查` / `不可用` / `未知` labels. Issues remain immediately after the health summary and the existing network topology is below it.
- `MetricCard` supports a presentation-only `StatusBadge`; the health grid starts as one column on small screens, becomes two columns, and reaches four columns on wide screens.
- Core health no longer equates `ComponentState::Available` with healthy. LAN requires confirmed LAN/AP readiness; Proxy requires consistent known Mihomo, LAN TUN, and local-system-proxy state; Tailscale requires desired/effective mode agreement and the relevant backend/authentication/route/firewall/approval state.
- Unknown runtime fields remain neutral instead of being promoted to green. Explicit mismatches become warning/error tones, while intentionally disabled but internally consistent Proxy/Tailscale states can still be healthy.
- Four deterministic browser-side unit tests cover LAN unknown/down readiness, Proxy three-part consistency, Tailscale mode/LAN-route readiness, and the invariant that a degraded top-level component cannot render as healthy.
- The obsolete `WORKSPACE_TABS`, `WORKSPACE_TAB`, and `WORKSPACE_TAB_ACTIVE` style tokens were removed after the AppShell/portal navigation migration made them unreachable.
- Refactor driver run `34491355789` passed migration, rustfmt, WASM `cargo check`, `git diff --check`, and generated the semantic health changes in `b923051104da967ccc544f92d6d704be7ed45c51`.
- The temporary driver now skips completed shared-component and first-pass Overview migrations by stage markers. This prevents earlier migration scripts from rewriting evolved component markup or failing on formatter-induced source layout changes.
- Human-authored head `3228801f1a13e11721d53bd1201ad0a11756a870`, CI run `34492752834`, passed the capability-suite gate, committed diff/static checks, contract format/tests/Clippy, deterministic frontend bundle, Rust formatting, all 13 browser-side web unit tests, native tests, and strict Clippy.
- The same run's Playwright job executed `Running 32 tests using 1 worker` and completed with `32 passed (2.3m)`. The healthy baseline explicitly checks all four core health cards, while the degraded Proxy journey separately checks `需检查`.

**Overview information-priority stage result: passed.** Plan section 9 is green.

## 2026-09-10 — Application shell and responsive navigation checkpoint

- The seven existing first-level destinations and `AppPage` state/ARIA relationships are unchanged; only AppShell presentation layout moved.
- On desktop, `主导航` is a sticky single-column rail to the left of active page content. On narrow/mobile viewports the same seven destinations render above content in a compact four-plus-three grid, while horizontal touch swipe remains available.
- Overview/Network mounted-state behavior, Camera stop-on-page-leave lifecycle, admin authorization boundaries, and mutation flows remain owned by the application layer and were not moved into `AppShell`.
- Existing `shell.spec.ts` journeys now use actual bounding boxes rather than CSS-class selectors: desktop navigation must be left of Overview and vertically stacked; at 360 px the navigation must be above content, Tailscale remains in row one, Camera begins row two, and there is no page-level horizontal overflow.
- Driver run `34494788296` on human head `9058db57eafdb363fddffdd0e15bf08fe28b26b4` passed migrations, rustfmt, WASM `cargo check`, and diff/commit gates without generating a follow-up commit.
- Full PR CI run `34494793676` on the same human head passed capability-suite/static checks, contract format/tests/Clippy, deterministic frontend bundle, all 13 browser-side unit tests, native tests, strict Clippy, and artifact verification.
- The same run's Playwright job executed `Running 32 tests using 1 worker` and completed with `32 passed (2.4m)`, so both new desktop and mobile geometry assertions passed in Chromium.

**Application-shell/navigation stage result: passed.** Plan section 8 is green.

## Current stage — Semantic light/dark theme and visual cleanup

- Keep Tailwind CSS 4 and daisyUI 5 as the only component/theming system.
- Replace the fixed Dracula-only root contract with daisyUI semantic themes: `light` is the default and `dracula` is selected by `prefers-color-scheme: dark`.
- Remove the hard-coded `data-theme="dracula"` and dark-only `color-scheme` declaration from the HTML shell. Browser theme-color metadata follows the operating-system light/dark preference.
- Remove fixed Dracula background/theme colors from the PWA manifest instead of publishing a single color as if it represented both themes.
- Keep components on semantic `base-*`, `primary`, `success`, `warning`, `error`, and related tokens; do not fork component markup by theme.
- Extend the existing shell E2E journey to switch Playwright media preference light → dark → light and verify the daisyUI `--color-base-300` token changes and restores. The number and capability organization of E2E suites remain unchanged.
- After semantic theme support is green, audit remaining heavy shadows/surfaces and responsive spacing as a separate visual-cleanup pass rather than changing behavior in the theme commit.
- **Pending checkpoint:** this theme-contract change must pass the complete human-authored PR CI, including the 32-test runnable Playwright baseline, before the semantic-theme portion of plan section 10 is marked green.
