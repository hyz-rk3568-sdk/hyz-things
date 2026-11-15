#!/usr/bin/env bash
# Print one selected Playwright spec per line. No output means the complete suite.
# Shared by the E2E count planner and every E2E runner.
set -Eeuo pipefail
readonly WORKSPACE="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../.." && pwd)"
if [[ "${GITHUB_EVENT_NAME:-}" != "pull_request" ]]; then
  exit 0
fi
if [[ -z "${GITHUB_BASE_REF:-}" ]]; then
  printf 'error: missing GITHUB_BASE_REF for PR test selection\n' >&2
  exit 1
fi
cd -- "$WORKSPACE"
changed="$(git -C "$WORKSPACE" -c core.quotepath=false diff --name-only "origin/$GITHUB_BASE_REF...HEAD")"

# Always include specs changed by the PR itself, even when the base selection
# above already names a broader set of related suites.
emit_selected() {
  local -a selected=("$@")
  local file
  while IFS= read -r file; do
    [[ "$file" == apps/rust/things/e2e/*.spec.ts ]] || continue
    selected+=("${file#apps/rust/things/}")
  done <<< "$changed"
  printf '%s\n' "${selected[@]}" | awk 'NF && !seen[$0]++'
}
# Scope this PR to the 申论 E2E. Shared code still goes through Rust CI.
# When unrelated paths are present, the existing wider selection is retained.
shenlun_scope='^((apps/rust/things/e2e/shenlun\.spec\.ts)|(apps/rust/things/e2e/helpers/shenlun\.ts)|(apps/rust/things/src/web/question_bank/(shenlun|ocr_panel|source_answer_panel|ai_composer|ai_review|ai_review_api|ai_review_recovery|ai_review_stream|ai_review_timeline|mod)\.rs)|(apps/rust/things/src/web/pages/(mod|training|scratchpad|material_scroll|question_navigation|training_setup|training_split_workspace|practice_status)\.rs)|(apps/rust/things/src/domain/question_bank\.rs)|(apps/rust/things/src/adapters/inbound/http/(mod|question_bank|shenlun)\.rs)|(apps/rust/things/src/adapters/outbound/(question_bank|shenlun)[^/]*\.rs)|(apps/rust/things/src/main\.rs)|(apps/rust/things/src/application/question_bank/(mod|shenlun|shenlun_grading|practice)\.rs)|(.agents/skills/hyz-shenlun/(SKILL\.md|references/scoring\.md))|(docs/plan/2026-11-13-shenlun-grading-specialized-training-plan\.md)|(apps/rust/things/tools/select-related-e2e\.sh))$'
if printf '%s\n' "$changed" | grep -qx 'apps/rust/things/e2e/shenlun.spec.ts'; then
  non_shenlun="$(printf '%s\n' "$changed" | grep -vE "$shenlun_scope" || true)"
  if [[ -z "$non_shenlun" ]]; then
    printf '%s\n' e2e/shenlun.spec.ts
    exit 0
  fi
fi

question_bank_scope='^((apps/rust/things/e2e/(question-bank(-ai|-drill|-history|-performance|-reference|-resume|-theme)?|shenlun|favorite-flashcard|flashcards)\.spec\.ts)|(apps/rust/things/e2e/helpers/(question-bank|shenlun)\.ts)|(apps/rust/things/e2e/(support|ink-fixtures|fixtures)\.ts)|(apps/rust/things/src/web/question_bank/[^/]+\.rs)|(apps/rust/things/src/domain/question_bank\.rs)|(apps/rust/things/src/application/question_bank/[^/]+\.rs)|(apps/rust/things/src/adapters/inbound/http/(mod|question_bank|shenlun)\.rs)|(apps/rust/things/src/adapters/outbound/(question_bank[^/]*|shenlun_ai)\.rs)|(apps/rust/things/src/main\.rs)|(apps/rust/things/tests/question_bank_drill\.rs)|(apps/rust/things/tools/(check-e2e-suites|frontend-bundle-cache|test-portal-ci)\.py)|(apps/rust/things/tools/select-related-e2e\.sh)|(question_bank/collections/xiaop-2026-shuliang/(06-行程问题（二）|07-溶液问题)\.md)|(docs/plan/2026-11-0[0-9]-shenlun[^/]*\.md)|(docs/plan/2026-11-1[0-9]-(shenlun|question-bank)[^/]*\.md)|(\.github/workflows/portal-ci\.yml))$'
if [[ -z "$(printf '%s\n' "$changed" | grep -vE "$question_bank_scope" || true)" ]]; then
  emit_selected \
    e2e/question-bank.spec.ts \
    e2e/question-bank-ai.spec.ts \
    e2e/question-bank-reference.spec.ts \
    e2e/shenlun.spec.ts \
    e2e/favorite-flashcard.spec.ts \
    e2e/question-bank-drill.spec.ts \
    e2e/flashcards.spec.ts
else
  emit_selected \
    e2e/question-bank.spec.ts \
    e2e/question-bank-ai.spec.ts \
    e2e/question-bank-reference.spec.ts \
    e2e/question-bank-drill.spec.ts \
    e2e/comprehensive.spec.ts \
    e2e/logic-judgment.spec.ts \
    e2e/training-subjects.spec.ts \
    e2e/training-time.spec.ts \
    e2e/training-width.spec.ts \
    e2e/shenlun.spec.ts \
    e2e/favorite-flashcard.spec.ts
fi
