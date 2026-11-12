#!/usr/bin/env python3
"""Guard Portal CI topology and exercise its check commands with fake compilers."""

from __future__ import annotations

import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[4]
WORKFLOW = ROOT / ".github/workflows/portal-ci.yml"


def named_blocks(text: str, pattern: str) -> dict[str, str]:
    matches = list(re.finditer(pattern, text, re.MULTILINE))
    blocks = {}
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        blocks[match.group(1)] = text[match.end():end]
    return blocks


class PortalCiTests(unittest.TestCase):
    def setUp(self):
        self.text = WORKFLOW.read_text()
        self.jobs = named_blocks(self.text.split("\njobs:\n", 1)[1], r"^  ([a-z][a-z0-9-]*):\n")

    def checks(self):
        self.assertIn("checks", self.jobs)
        return named_blocks(self.jobs["checks"], r"^      - name: ([^\n]+)\n")

    def preflight(self):
        self.assertIn("preflight", self.jobs)
        return named_blocks(self.jobs["preflight"], r"^      - name: ([^\n]+)\n")

    def test_format_static_and_unit_checks_are_independent_failure_collecting_jobs(self):
        preflight, checks = self.preflight(), self.checks()
        for steps, names in (
            (preflight, ("Check contract formatting", "Check things formatting",
                         "Run source and configuration checks", "Validate capability Playwright suite layout",
                         "Check committed diff")),
            (checks, ("Test contract", "Test native", "Test web unit logic",
                      "Clippy contract", "Clippy native")),
        ):
            for name in names:
                with self.subTest(step=name):
                    self.assertIn(name, steps)
                    self.assertIn("!cancelled()", steps[name])
        self.assertNotIn("continue-on-error:", self.text)

    def test_failed_formatting_and_static_checks_still_run_all_unit_suites(self):
        steps = {**self.preflight(), **self.checks()}
        with tempfile.TemporaryDirectory(prefix="hyz-ci-test-") as temporary:
            directory = Path(temporary)
            log = directory / "commands.log"
            for tool in ("cargo", "make"):
                executable = directory / tool
                executable.write_text('''#!/bin/bash
printf '%s %s\\n' "${0##*/}" "$*" >> "$FAKE_CI_LOG"
if [[ "${0##*/}" == make || "${1:-}" == fmt ]]; then exit 1; fi
''')
                executable.chmod(0o755)
            environment = dict(os.environ, PATH=f"{directory}:{os.environ['PATH']}", FAKE_CI_LOG=str(log))
            failed = []
            for name, step in steps.items():
                command = re.search(r"^        run: ((?:cargo|make|timeout) [^\n]+)$", step, re.MULTILINE)
                if command:
                    self.assertIn("!cancelled()", step)
                    result = subprocess.run(["bash", "-e", "-o", "pipefail"], input=command.group(1),
                                            text=True, env=environment, capture_output=True, timeout=10)
                    if result.returncode:
                        failed.append(name)
            calls = log.read_text().splitlines()
            self.assertEqual(len(failed), 3, failed)
            self.assertEqual(sum(line.startswith("cargo test ") for line in calls), 3, calls)
            self.assertEqual(sum(line.startswith("cargo clippy ") for line in calls), 2, calls)

    def test_e2e_and_checks_depend_only_on_frontend(self):
        self.assertEqual(set(self.jobs), {"frontend", "preflight", "e2e-plan", "checks", "e2e", "e2e-summary"})
        for name in ("frontend", "preflight", "e2e-plan"):
            self.assertNotRegex(self.jobs[name], r"(?m)^    needs:")
        self.assertRegex(self.jobs["checks"], r"(?m)^    needs: frontend$")
        self.assertIn("needs: [frontend, e2e-plan]", self.jobs["e2e"] )
        self.assertRegex(self.jobs["e2e-summary"], r"(?m)^    needs: e2e$")
        self.assertIn("!cancelled()", self.jobs["checks"])

    def test_frontend_cache_and_artifact_include_fingerprint_metadata(self):
        frontend = self.jobs["frontend"]
        self.assertIn("frontend-bundle-cache.py fingerprint", frontend)
        self.assertIn("steps.frontend_inputs.outputs.sha", frontend)
        self.assertIn("target/frontend-bundle/hyz-things-frontend.tar", frontend)
        self.assertIn("target/frontend-bundle/hyz-things-frontend.json", frontend)


    def test_e2e_installs_webkit_and_uses_the_default_browser(self):
        steps = named_blocks(self.jobs["e2e"], r"^      - name: ([^\n]+)\n")
        self.assertIn("Install Playwright WebKit", steps)
        self.assertIn("npx playwright install --with-deps webkit", steps["Install Playwright WebKit"])
        e2e = steps["Run Playwright E2E"]
        # The PR command is selected by changed file scope, while pushes to
        # main still run the full browser suite on the default WebKit browser.
        self.assertIn("run: |", e2e)
        self.assertIn('selected="$(bash tools/select-related-e2e.sh)"', e2e)
        self.assertIn('npm run test:e2e -- "${specs[@]}"', e2e)
        selection = (ROOT / "apps/rust/things/tools/select-related-e2e.sh").read_text()
        self.assertIn('git -C "$WORKSPACE" -c core.quotepath=false diff --name-only', selection)
        for spec in ("question-bank.spec.ts", "favorite-flashcard.spec.ts",
                     "question-bank-drill.spec.ts", "flashcards.spec.ts",
                     "comprehensive.spec.ts", "logic-judgment.spec.ts"):
            self.assertIn(f"e2e/{spec}", selection)
        self.assertNotIn("HYZ_THINGS_E2E_BROWSER=", e2e)

    def test_adaptive_e2e_shards_scale_to_four_runners(self):
        planner = self.jobs["e2e-plan"]
        e2e = self.jobs["e2e"]
        self.assertIn("playwright test", planner)
        self.assertIn("--list", planner)
        for threshold in ("count > 60", "count > 120", "count > 180"):
            self.assertIn(threshold, planner)
        for shards in ("[1]", "[1,2]", "[1,2,3]", "[1,2,3,4]"):
            self.assertIn(shards, planner)
        self.assertIn("fail-fast: false", e2e)
        self.assertIn("max-parallel: 4", e2e)
        self.assertIn("shard: ${{ fromJSON(needs.e2e-plan.outputs.shards) }}", e2e)
        self.assertIn("E2E_SHARD_TOTAL: ${{ needs.e2e-plan.outputs.total }}", e2e)
        self.assertIn('--shard="${E2E_SHARD}/${E2E_SHARD_TOTAL}"', e2e)
        self.assertIn("shard-${{ matrix.shard }}", e2e)
        self.assertIn("test '${{ needs.e2e.result }}' = 'success'", self.jobs["e2e-summary"])

    def test_selection_script_runs_without_a_github_event(self):
        script = ROOT / "apps/rust/things/tools/select-related-e2e.sh"
        syntax = subprocess.run(["bash", "-n", str(script)], capture_output=True, text=True)
        self.assertEqual(syntax.returncode, 0, syntax.stderr)
        env = dict(os.environ, GITHUB_EVENT_NAME="push")
        selection = subprocess.run(["bash", str(script)], capture_output=True, text=True, env=env)
        self.assertEqual(selection.returncode, 0, selection.stderr)
        self.assertEqual(selection.stdout, "")

    def test_verified_cache_hit_skips_dependency_target_and_bundle_build(self):
        steps = named_blocks(self.jobs["frontend"], r"^      - name: ([^\n]+)\n")
        self.assertIn("frontend-bundle-cache.py check", steps["Validate cached frontend bundle"])
        for name in ("Install frontend dependencies", "Install WASM target", "Build deterministic frontend bundle"):
            self.assertIn("if: steps.frontend_cache.outputs.hit != 'true'", steps[name])
        self.assertIn("steps.frontend_inputs.outputs.tools", steps["Cache verified frontend bundle"])


if __name__ == "__main__":
    unittest.main()
