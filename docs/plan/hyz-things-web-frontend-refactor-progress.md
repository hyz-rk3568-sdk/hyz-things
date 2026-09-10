# hyz-things Web frontend refactor progress

This file records implementation evidence while `hyz-things-web-frontend-refactor-plan.md` is being executed. The final Host matrix must still be copied back into the plan before the refactor is marked complete.

## 2026-09-10 — structural checkpoint

- Remaining page ownership has been extracted into `pages/overview.rs` and `pages/system.rs`.
- `pages/mod.rs` and `hooks/mod.rs` now provide the normal module roots instead of root-level `#[path]` aliases.
- `app.rs` owns portal composition/navigation/reducer orchestration; the System arm delegates page rendering to `pages/system.rs`.
- The structural driver run `34474799636` passed migration, rustfmt, WASM `cargo check`, `git diff --check`, and generated-commit gates.
- The preceding full CI run `34472739509` passed static/native/strict-Clippy/frontend checks and had exactly two Playwright migration failures. Both were test-contract defects: the PiP helper added display fields without unit conversion, and the administrator journey contained a duplicated navigation insertion that looked for the Network STA control on the Proxy page.
- Those two Playwright defects are fixed in the structural checkpoint commit.

Next required gate before shared components: run the complete `hyz-things CI` on a human-authored branch head and require the existing Playwright suite to pass without deleting business assertions.
