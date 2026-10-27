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

    def test_format_static_and_unit_checks_share_one_failure_collecting_job(self):
        steps = self.checks()
        for name in ("Check contract formatting", "Check things formatting", "Run source and configuration checks",
                     "Test contract", "Test native", "Test web unit logic", "Clippy contract", "Clippy native"):
            with self.subTest(step=name):
                self.assertIn(name, steps)
                self.assertIn("!cancelled()", steps[name])
        self.assertNotIn("continue-on-error:", self.text)

    def test_failed_formatting_and_static_checks_still_run_all_unit_suites(self):
        steps = self.checks()
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
                command = re.search(r"^        run: ((?:cargo|make) [^\n]+)$", step, re.MULTILINE)
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
        self.assertEqual(set(self.jobs), {"frontend", "checks", "e2e"})
        self.assertNotRegex(self.jobs["frontend"], r"(?m)^    needs:")
        for name in ("checks", "e2e"):
            self.assertRegex(self.jobs[name], r"(?m)^    needs: frontend$")
        self.assertIn("!cancelled()", self.jobs["checks"])

    def test_frontend_cache_and_artifact_include_fingerprint_metadata(self):
        frontend = self.jobs["frontend"]
        self.assertIn("frontend-bundle-cache.py fingerprint", frontend)
        self.assertIn("steps.frontend_inputs.outputs.sha", frontend)
        self.assertIn("target/frontend-bundle/hyz-things-frontend.tar", frontend)
        self.assertIn("target/frontend-bundle/hyz-things-frontend.json", frontend)


    def test_verified_cache_hit_skips_dependency_target_and_bundle_build(self):
        steps = named_blocks(self.jobs["frontend"], r"^      - name: ([^\n]+)\n")
        self.assertIn("frontend-bundle-cache.py check", steps["Validate cached frontend bundle"])
        for name in ("Install frontend dependencies", "Install WASM target", "Build deterministic frontend bundle"):
            self.assertIn("if: steps.frontend_cache.outputs.hit != 'true'", steps[name])
        self.assertIn("steps.frontend_inputs.outputs.tools", steps["Cache verified frontend bundle"])


if __name__ == "__main__":
    unittest.main()
