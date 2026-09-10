#!/usr/bin/env python3
'''Split the monolithic Playwright portal suite by product capability.

This temporary migration preserves each existing test block byte-for-byte while
moving shared helpers into support.ts. The split is idempotent and validates
that no test is lost or duplicated.
'''

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[4]
E2E = ROOT / "apps/rust/things/e2e"
PORTAL = E2E / "portal.spec.ts"
SUPPORT = E2E / "support.ts"

SPEC_FILES = {
    "shell": E2E / "shell.spec.ts",
    "countdown": E2E / "countdown.spec.ts",
    "security": E2E / "security.spec.ts",
    "camera": E2E / "camera.spec.ts",
    "proxy": E2E / "proxy.spec.ts",
    "network": E2E / "network.spec.ts",
    "tailscale": E2E / "tailscale.spec.ts",
}

HELPERS = (
    "swipePortal",
    "realTouchSwipe",
    "readCountdownTotal",
    "readCustomCountdownTotal",
)

TEST_START = re.compile(r'(?m)^test\("([^"]+)"')
EXPECTED_MARKER = re.compile(r"(?m)^// expected-tests: (\d+)$")

SPEC_PREAMBLE = '''import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import {
  expectNoHorizontalOverflow,
  goToAppPage,
  harnessOrigin,
  installCameraWebRtcMock,
  loginAsAdmin,
  readHarnessState,
  resetHarness,
  webOrigin,
} from "./fixtures";
import {
  readCountdownTotal,
  readCustomCountdownTotal,
  realTouchSwipe,
  swipePortal,
} from "./support";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

'''


def function_block(text: str, name: str) -> tuple[int, int, str]:
    marker = f"async function {name}"
    start = text.find(marker)
    if start < 0:
        raise SystemExit(f"helper not found: {name}")
    brace = text.find("{", start)
    if brace < 0:
        raise SystemExit(f"helper opening brace not found: {name}")

    depth = 0
    index = brace
    while index < len(text):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                end = index + 1
                if end < len(text) and text[end] == ";":
                    end += 1
                while end < len(text) and text[end] == "\n":
                    end += 1
                return start, end, text[start:index + 1]
        index += 1
    raise SystemExit(f"helper closing brace not found: {name}")


def extract_helpers(text: str) -> tuple[str, list[str]]:
    ranges = []
    exported = []
    for name in HELPERS:
        start, end, block = function_block(text, name)
        ranges.append((start, end))
        exported.append(block.replace("async function", "export async function", 1))

    for start, end in sorted(ranges, reverse=True):
        text = text[:start] + text[end:]
    return text, exported


def split_tests(text: str) -> list[tuple[str, str]]:
    starts = list(TEST_START.finditer(text))
    if not starts:
        raise SystemExit("no Playwright tests found in portal.spec.ts")

    tests = []
    for index, match in enumerate(starts):
        start = match.start()
        end = starts[index + 1].start() if index + 1 < len(starts) else len(text)
        block = text[start:end].strip()
        tests.append((match.group(1), block))
    return tests


def category_for(title: str) -> str:
    lower = title.lower()
    if "countdown" in lower or "picture-in-picture" in lower or "exam " in lower:
        return "countdown"
    if "camera" in lower or "intercom" in lower or "viewers" in lower:
        return "camera"
    if "tailscale" in lower or "tailnet" in lower:
        return "tailscale"
    if "proxy" in lower:
        return "proxy"
    if "STA" in title or " AP" in title or "subscription" in lower:
        return "network"
    if "http boundary" in lower:
        return "security"
    return "shell"


def validate_split(expected: int | None = None) -> None:
    if PORTAL.exists():
        raise SystemExit("portal.spec.ts still exists after capability split")
    if not SUPPORT.exists():
        raise SystemExit("support.ts missing after capability split")
    missing = [path.name for path in SPEC_FILES.values() if not path.exists()]
    if missing:
        raise SystemExit("missing split specs: " + ", ".join(missing))

    support = SUPPORT.read_text()
    marker = EXPECTED_MARKER.search(support)
    if marker is None:
        raise SystemExit("support.ts expected-test marker missing")
    marker_count = int(marker.group(1))
    if expected is not None and marker_count != expected:
        raise SystemExit(
            f"expected-test marker mismatch: marker={marker_count} source={expected}"
        )

    titles = []
    per_file = {}
    for category, path in SPEC_FILES.items():
        text = path.read_text()
        matches = TEST_START.findall(text)
        if not matches:
            raise SystemExit(f"{path.name}: empty capability suite")
        if "test.beforeEach" not in text or "resetHarness(request)" not in text:
            raise SystemExit(f"{path.name}: harness reset missing")
        titles.extend(matches)
        per_file[category] = len(matches)

    if len(titles) != marker_count:
        raise SystemExit(
            f"split test count changed: marker={marker_count} actual={len(titles)}"
        )
    if len(set(titles)) != len(titles):
        duplicates = sorted({title for title in titles if titles.count(title) > 1})
        raise SystemExit("duplicate split test titles: " + ", ".join(duplicates))

    print(
        "validated capability Playwright suites: "
        + ", ".join(f"{name}={per_file[name]}" for name in SPEC_FILES)
        + f"; total={len(titles)}"
    )


def migrate() -> None:
    if not PORTAL.exists():
        validate_split()
        return

    source = PORTAL.read_text()
    source_without_helpers, helpers = extract_helpers(source)
    tests = split_tests(source_without_helpers)
    titles = [title for title, _ in tests]
    if len(set(titles)) != len(titles):
        raise SystemExit("portal.spec.ts contains duplicate test titles")

    grouped = {category: [] for category in SPEC_FILES}
    for title, block in tests:
        grouped[category_for(title)].append(block)

    empty = [category for category, blocks in grouped.items() if not blocks]
    if empty:
        raise SystemExit("capability split produced empty suites: " + ", ".join(empty))

    SUPPORT.write_text(
        f"// expected-tests: {len(tests)}\n"
        'import type { Page } from "@playwright/test";\n\n'
        + "\n\n".join(helpers)
        + "\n"
    )
    for category, path in SPEC_FILES.items():
        path.write_text(SPEC_PREAMBLE + "\n\n".join(grouped[category]) + "\n")

    PORTAL.unlink()
    validate_split(expected=len(tests))


migrate()
