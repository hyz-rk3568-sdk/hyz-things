#!/usr/bin/env python3
"""Validate the capability-oriented Playwright suite layout."""

from collections import Counter
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[4]
E2E = ROOT / "apps/rust/things/e2e"
SUPPORT = E2E / "support.ts"
PORTAL = E2E / "portal.spec.ts"

REQUIRED_SPECS = (
    "shell.spec.ts",
    "countdown.spec.ts",
    "security.spec.ts",
    "camera.spec.ts",
    "proxy.spec.ts",
    "network.spec.ts",
    "tailscale.spec.ts",
)
TEST_TITLE = re.compile(r'(?m)^test\("([^"]+)"')
EXPECTED_MARKER = re.compile(r"(?m)^// expected-tests: (\d+)$")


def fail(message: str) -> None:
    raise SystemExit(f"E2E suite layout invalid: {message}")


def main() -> None:
    if PORTAL.exists():
        fail("portal.spec.ts must not return after the capability split")
    if not SUPPORT.is_file():
        fail("support.ts is missing")

    actual_specs = tuple(sorted(path.name for path in E2E.glob("*.spec.ts")))
    expected_specs = tuple(sorted(REQUIRED_SPECS))
    if actual_specs != expected_specs:
        missing = sorted(set(expected_specs) - set(actual_specs))
        unexpected = sorted(set(actual_specs) - set(expected_specs))
        fail(f"suite set changed; missing={missing}, unexpected={unexpected}")

    support = SUPPORT.read_text()
    marker = EXPECTED_MARKER.search(support)
    if marker is None:
        fail("support.ts expected-test marker is missing")
    expected_total = int(marker.group(1))

    titles: list[str] = []
    counts: dict[str, int] = {}
    for name in REQUIRED_SPECS:
        text = (E2E / name).read_text()
        suite_titles = TEST_TITLE.findall(text)
        if not suite_titles:
            fail(f"{name} contains no top-level Playwright tests")
        if "test.beforeEach" not in text or "resetHarness(request)" not in text:
            fail(f"{name} does not reset the E2E harness per test")
        titles.extend(suite_titles)
        counts[name] = len(suite_titles)

    duplicates = sorted(title for title, count in Counter(titles).items() if count > 1)
    if duplicates:
        fail("duplicate test titles: " + ", ".join(duplicates))
    if len(titles) != expected_total:
        fail(f"expected-test marker={expected_total}, discovered={len(titles)}")

    print(
        "capability Playwright suites valid: "
        + ", ".join(f"{name}={counts[name]}" for name in REQUIRED_SPECS)
        + f"; total={len(titles)}"
    )


if __name__ == "__main__":
    main()
