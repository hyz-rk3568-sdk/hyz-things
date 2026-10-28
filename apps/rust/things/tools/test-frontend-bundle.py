#!/usr/bin/env python3
"""Host-only frontend bundle tests; all compiler/build commands are fakes."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parent

FAKE_TOOL = r'''#!/usr/bin/env python3
import os
from pathlib import Path
import sys

name = Path(sys.argv[0]).name
args = sys.argv[1:]
log = Path(os.environ["FAKE_BUILD_LOG"])
if args in (["-vV"], ["--version"]) and os.environ.get("FAKE_EXPECT_TOOL_CWD"):
    if Path.cwd() != Path(os.environ["FAKE_EXPECT_TOOL_CWD"]):
        raise SystemExit("toolchain probed from the wrong directory")
if os.environ.get("FAKE_FORBID_BUILD_TOOLS") and name != "cargo":
    raise SystemExit("build tools must not be queried")
def record(message):
    with log.open("a") as output:
        output.write(message + "\n")

if name == "node":
    print("v22.0.0" if args == ["--version"] else "22")
elif name == "npm" and args == ["--version"]:
    print("10.0.0")
elif name == "npm" and args == ["run", "build:css"]:
    record("npm build:css")
    Path("frontend/router.css").write_text("/* generated */")
elif name == "rustup" and args == ["target", "list", "--installed"]:
    print("wasm32-unknown-unknown")
elif name == "rustc" and args in (["-vV"], ["--version", "--verbose"]):
    print(os.environ.get("FAKE_RUST_VERSION", "rustc 1.88.0"))
elif name == "trunk" and args == ["--version"]:
    print(os.environ.get("FAKE_TRUNK_VERSION", "trunk 0.21.14"))
elif name == "trunk" and args and args[0] == "build":
    record("trunk build")
    if os.environ.get("FAKE_BUILD_GATE"):
        print("FAKE_BUILD_WAITING", flush=True)
        Path(os.environ["FAKE_BUILD_GATE"]).read_bytes()
    if os.environ.get("FAKE_FAIL_BUILD"):
        raise SystemExit(17)
    dist = Path(args[args.index("--dist") + 1])
    dist.mkdir(parents=True)
    revision = os.environ.get("FAKE_OUTPUT_REVISION", "one")
    (dist / "index.html").write_text('<!doctype html><html><head><link rel="stylesheet" href="/app.css">'
        '<script type="module">import init from "/app.js"; await init();</script></head><body></body></html>')
    (dist / "app.css").write_text(".app {color: black}")
    (dist / "app.js").write_text("// " + revision)
    if os.environ.get("FAKE_MUTATE_INPUT"):
        Path(os.environ["FAKE_MUTATE_INPUT"]).write_text("changed during build")
elif name == "cargo" and args and args[0] == "run":
    record("cargo run")
else:
    raise SystemExit("unexpected fake invocation: " + name + " " + repr(args))
'''


class FrontendBundleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="hyz-frontend-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.package = self.root / "apps/rust/things"
        self.tools = self.package / "tools"
        self.tools.mkdir(parents=True)
        for name in ("build-frontend-bundle.sh", "start-e2e-server.sh",
                     "externalize-trunk-bootstrap.py", "frontend-bundle-cache.py"):
            if (TOOLS / name).exists():
                shutil.copy2(TOOLS / name, self.tools / name)
        for name in ("Cargo.toml", "Cargo.lock", "package.json", "package-lock.json", "Trunk.toml", "build.rs"):
            shutil.copy2(TOOLS.parent / name, self.package / name)
        self.frontend = self.package / "frontend"
        self.frontend.mkdir()
        for name in ("index.html", "app.css", "katex-render.js"):
            shutil.copy2(TOOLS.parent / "frontend" / name, self.frontend / name)
        self.source = self.package / "src/web/main.rs"
        self.source.parent.mkdir(parents=True)
        self.source.write_text("fn main() {}\n")
        contract = self.root / "apps/rust/contract"
        (contract / "src").mkdir(parents=True)
        (contract / "src/lib.rs").write_text("// contract\n")
        for name in ("Cargo.toml", "Cargo.lock"):
            shutil.copy2(TOOLS.parent.parent / "contract" / name, contract / name)
        tailwind = self.package / "node_modules/.bin/tailwindcss"
        tailwind.parent.mkdir(parents=True)
        tailwind.write_text("#!/bin/sh\nexit 0\n")
        tailwind.chmod(0o755)
        fake_bin = self.root / "fake-bin"
        fake_bin.mkdir()
        for name in ("node", "npm", "rustup", "rustc", "trunk", "cargo"):
            executable = fake_bin / name
            executable.write_text(FAKE_TOOL)
            executable.chmod(0o755)
        self.log = self.root / "build.log"
        self.environment = dict(os.environ, PATH=f"{fake_bin}:{os.environ['PATH']}",
                                FAKE_BUILD_LOG=str(self.log), CARGO_HOME=str(self.root / "cargo-home"))
        for name in ("HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE", "FAKE_FAIL_BUILD",
                     "FAKE_MUTATE_INPUT", "FAKE_OUTPUT_REVISION", "FAKE_BUILD_GATE"):
            self.environment.pop(name, None)
        self.archive = self.root / "target/frontend-bundle/hyz-things-frontend.tar"
        self.metadata = self.archive.with_suffix(".json")

    def run_script(self, name="build-frontend-bundle.sh", *, success=True, **environment):
        result = subprocess.run(["bash", str(self.tools / name)], env=dict(self.environment, **environment),
                                capture_output=True, text=True, timeout=30)
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def build_calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def run_prebuilt(self, *, success=True):
        return self.run_script("start-e2e-server.sh", success=success,
                               HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE="1")

    def stamp_archive(self):
        timestamp = 1_700_000_000_123_456_789
        os.utime(self.archive, ns=(timestamp, timestamp))
        return timestamp

    def test_unchanged_inputs_reuse_without_running_build_tools(self):
        self.run_script()
        self.assertTrue(self.archive.is_file())
        first_calls = self.build_calls()
        self.assertIn("trunk build", first_calls)
        self.run_script()
        self.assertEqual(self.build_calls(), first_calls)


    def test_changed_compiler_invalidates_local_reuse(self):
        self.run_script()
        self.run_script(FAKE_RUST_VERSION="rustc 1.89.0")
        self.assertEqual(self.build_calls().count("trunk build"), 2)


    def test_explicit_e2e_reuse_refuses_changed_source_before_starting_cargo(self):
        self.run_script()
        self.source.write_text("fn main() { /* changed */ }\n")
        self.run_prebuilt(success=False)
        self.assertNotIn("cargo run", self.build_calls())


    def test_tailwind_scanned_test_sources_invalidate_reuse(self):
        test_source = self.package / "e2e/example.spec.ts"
        test_source.parent.mkdir()
        test_source.write_text('const classes = "bg-purple-950";')
        self.run_script()
        test_source.write_text('const classes = "bg-cyan-800";')
        self.run_script()
        self.assertEqual(self.build_calls().count("trunk build"), 2)


    def test_identical_rebuilt_archive_preserves_mtime(self):
        self.run_script()
        archive = self.archive.read_bytes()
        timestamp = self.stamp_archive()
        self.source.write_text("fn main() { /* same generated output */ }\n")
        self.run_script()
        self.assertEqual(self.build_calls().count("trunk build"), 2)
        self.assertEqual(self.archive.read_bytes(), archive)
        self.assertEqual(self.archive.stat().st_mtime_ns, timestamp)


    def test_boolean_cache_version_is_rejected(self):
        self.run_script()
        metadata = json.loads(self.metadata.read_text())
        metadata["version"] = True
        self.metadata.write_text(json.dumps(metadata))
        self.run_prebuilt(success=False)
        self.assertNotIn("cargo run", self.build_calls())

    def test_source_lock_asset_and_config_changes_rebuild(self):
        self.run_script()
        paths = [self.source, self.root / "apps/rust/contract/src/lib.rs",
                 self.package / "Cargo.lock", self.package / "package-lock.json",
                 self.frontend / "index.html", self.frontend / "app.css",
                 self.frontend / "pwa-icon.svg", self.package / "Trunk.toml",
                 self.root / ".cargo/config.toml"]
        for index, path in enumerate(paths, start=2):
            with self.subTest(path=path.relative_to(self.root)):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text((path.read_text() if path.exists() else "") + "\n# changed\n")
                self.run_script()
                self.assertEqual(self.build_calls().count("trunk build"), index)

    def test_source_mtime_and_generated_outputs_do_not_invalidate(self):
        self.run_script()
        calls = self.build_calls()
        os.utime(self.source, ns=(1_700_000_000_000_000_000,) * 2)
        for path in [self.frontend / "router.css", self.package / "target/cache",
                     self.package / "node_modules/cache", self.package / "test-results/result",
                     self.package / "playwright-report/report", self.package / "tools/__pycache__/cache.pyc"]:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("generated output")
        self.run_script()
        self.assertEqual(self.build_calls(), calls)

    def test_new_and_deleted_sources_invalidate(self):
        self.run_script()
        source = self.source.with_name("new_module.rs")
        source.write_text("// new module")
        self.run_script()
        source.unlink()
        self.run_script()
        self.assertEqual(self.build_calls().count("trunk build"), 3)

    def test_tampered_archive_is_refused_before_cargo_and_rebuilt(self):
        self.run_script()
        archive = self.archive.read_bytes()
        self.assertIn(b"// one", archive)
        self.archive.write_bytes(archive.replace(b"// one", b"// bad", 1))
        self.run_prebuilt(success=False)
        self.assertNotIn("cargo run", self.build_calls())
        self.run_script()
        self.assertEqual(self.archive.read_bytes(), archive)
        self.assertEqual(self.build_calls().count("trunk build"), 2)

    def test_missing_and_malformed_metadata_are_never_reused(self):
        self.run_script()
        for content in [None, "{", "[]", '{"version":99}', '{"version":1,"inputs_sha256":7}']:
            with self.subTest(content=content):
                self.metadata.unlink(missing_ok=True)
                if content is not None:
                    self.metadata.write_text(content)
                self.run_prebuilt(success=False)
        self.assertNotIn("cargo run", self.build_calls())

    def test_prebuilt_reuse_does_not_require_frontend_build_tools(self):
        self.run_script()
        self.run_script("start-e2e-server.sh", HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE="1",
                        FAKE_FORBID_BUILD_TOOLS="1")
        self.assertEqual(self.build_calls(), ["npm build:css", "trunk build", "cargo run"])

    def test_default_e2e_start_uses_validated_cache(self):
        self.run_script()
        self.run_script("start-e2e-server.sh")
        self.assertEqual(self.build_calls(), ["npm build:css", "trunk build", "cargo run"])

    def test_changed_archive_is_replaced(self):
        self.run_script()
        archive = self.archive.read_bytes()
        timestamp = self.stamp_archive()
        self.source.write_text("// changed output")
        self.run_script(FAKE_OUTPUT_REVISION="two")
        self.assertNotEqual(self.archive.read_bytes(), archive)
        self.assertNotEqual(self.archive.stat().st_mtime_ns, timestamp)
        self.run_prebuilt()

    def test_build_failure_keeps_previous_archive_and_metadata(self):
        self.run_script()
        archive, metadata = self.archive.read_bytes(), self.metadata.read_bytes()
        self.source.write_text("// changed")
        self.run_script(success=False, FAKE_FAIL_BUILD="1")
        self.assertEqual(self.archive.read_bytes(), archive)
        self.assertEqual(self.metadata.read_bytes(), metadata)

    def test_inputs_changed_during_build_are_not_certified(self):
        self.run_script()
        metadata = self.metadata.read_bytes()
        self.source.write_text("// rebuild")
        self.run_script(success=False, FAKE_MUTATE_INPUT=str(self.source))
        self.assertEqual(self.metadata.read_bytes(), metadata)
        self.run_prebuilt(success=False)
        self.assertNotIn("cargo run", self.build_calls())

    def test_missing_required_input_fails_before_build(self):
        (self.package / "Cargo.lock").unlink()
        self.run_script(success=False)
        self.assertEqual(self.build_calls(), [])

    def test_changed_rustflags_invalidates_local_reuse(self):
        self.run_script()
        self.run_script(RUSTFLAGS="-C opt-level=2")
        self.assertEqual(self.build_calls().count("trunk build"), 2)


    def test_build_holds_an_exclusive_output_lock(self):
        gate = self.root / "build-gate"
        os.mkfifo(gate)
        environment = dict(self.environment, FAKE_BUILD_GATE=str(gate))
        with subprocess.Popen(["bash", str(self.tools / "build-frontend-bundle.sh")], env=environment,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as process:
            marker = process.stdout.readline()
            if marker != "FAKE_BUILD_WAITING\n":
                stdout, stderr = process.communicate(timeout=10)
                self.fail(marker + stdout + stderr)
            try:
                lock = subprocess.run(["flock", "-n", str(self.archive.parent / ".build.lock"), "true"], timeout=10)
                self.assertNotEqual(lock.returncode, 0)
            finally:
                gate.write_text("release")
                stdout, stderr = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 0, stdout + stderr)


    def test_toolchain_is_probed_from_the_build_directory(self):
        self.run_script(FAKE_EXPECT_TOOL_CWD=str(self.package))


if __name__ == "__main__":
    unittest.main()
