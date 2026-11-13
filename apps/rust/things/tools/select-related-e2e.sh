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
unrelated="$(printf '%s\n' "$changed" | grep -vE '^((apps/rust/things/src/web/pages/(question_navigation|favorite_flashcard|question_bank_practice|collection_review|training|training_statistics)\.rs)|(apps/rust/things/e2e/(question-bank|favorite-flashcard|question-bank-drill|flashcards)\.spec\.ts)|(apps/rust/things/e2e/support\.ts)|(apps/rust/things/tests/question_bank_drill\.rs)|(apps/rust/things/tools/(check-e2e-suites|frontend-bundle-cache|test-portal-ci)\.py)|(question_bank/collections/xiaop-2026-shuliang/(06-行程问题（二）|07-溶液问题)\.md)|(\.github/workflows/portal-ci\.yml))$' || true)"
if [[ -z "$unrelated" ]]; then
  printf '%s\n' \
    e2e/question-bank.spec.ts \
    e2e/favorite-flashcard.spec.ts \
    e2e/question-bank-drill.spec.ts \
    e2e/flashcards.spec.ts
else
  printf '%s\n' \
    e2e/question-bank.spec.ts \
    e2e/question-bank-drill.spec.ts \
    e2e/comprehensive.spec.ts \
    e2e/logic-judgment.spec.ts \
    e2e/training-subjects.spec.ts \
    e2e/training-time.spec.ts \
    e2e/training-width.spec.ts \
    e2e/shenlun.spec.ts \
    e2e/favorite-flashcard.spec.ts
fi
