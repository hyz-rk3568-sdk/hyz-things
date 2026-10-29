#!/usr/bin/env python3
"""Exercise Playwright startup with a stalled loopback HTTP connection."""

from pathlib import Path
import json
import os
import shlex
import socket
import subprocess
import tempfile
import unittest

PACKAGE = Path(__file__).resolve().parents[1]
READY_RECORD = {
    "frontend_tar": "fixture.tar",
    "harness_control_origin": "http://127.0.0.1:31902",
    "web_origin": "http://127.0.0.1:31901",
}


def server_output(record):
    return f"print({json.dumps(record, separators=(',', ':'))!r}, flush=True)\n"


class E2eStartupTests(unittest.TestCase):
    def run_startup(self, server_source, assertions="", browser=None):
        with tempfile.TemporaryDirectory(prefix="hyz-e2e-startup-") as temporary:
            directory = Path(temporary)
            hook = directory / "stalled-http.cjs"
            hook.write_text("""
const http = require('node:http');
const { EventEmitter } = require('node:events');
const original = http.request;
http.request = function (url, ...args) {
  if (String(url).includes('127.0.0.1:31901/api/v1/health')) {
    const request = new EventEmitter();
    const timer = setInterval(() => {}, 1000);
    request.end = request.setTimeout = () => request;
    request.destroy = request.abort = () => clearInterval(timer);
    return request;
  }
  return original.call(this, url, ...args);
};
""")
            server = directory / "server.py"
            server.write_text(server_source + "\nimport time\nwhile True:\n    time.sleep(60)\n")
            config = directory / "playwright.config.ts"
            config.write_text(f"""
import config from {json.dumps(str(PACKAGE / 'playwright.config.ts'))};
export default {{
  ...config,
  testDir: {json.dumps(str(directory))},
  reporter: 'line',
  webServer: {{
    ...config.webServer,
    command: {json.dumps('python3 ' + shlex.quote(str(server)))},
    timeout: 1000,
  }},
}};
""")
            (directory / "startup.spec.ts").write_text(f"""
import {{ test, expect }} from {json.dumps(str(PACKAGE / 'node_modules/@playwright/test'))};
test('server startup completes', async ({{ browserName, hasTouch, isMobile }}) => {{ {assertions} }});
""")
            environment = dict(os.environ)
            environment.pop("HYZ_THINGS_E2E_BROWSER", None)
            if browser is not None:
                environment["HYZ_THINGS_E2E_BROWSER"] = browser
            environment.update(
                HYZ_THINGS_E2E_WEB_PORT="31901",
                HYZ_THINGS_E2E_CONTROL_PORT="31902",
                NODE_OPTIONS=f"{environment.get('NODE_OPTIONS', '')} --require={hook}",
            )
            try:
                return subprocess.run(
                    ["node", str(PACKAGE / "node_modules/@playwright/test/cli.js"),
                     "test", "--config", str(config)],
                    cwd=directory, env=environment, capture_output=True, text=True, timeout=10,
                )
            except subprocess.TimeoutExpired:
                self.fail("Playwright exceeded the bounded server startup wait")

    def test_ready_output_starts_tests_without_waiting_for_loopback_tcp_timeout(self):
        result = self.run_startup(server_output(READY_RECORD))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("1 passed", result.stdout)

    def test_default_browser_is_webkit_with_ipad_touch_emulation(self):
        result = self.run_startup(server_output(READY_RECORD),
                                  "expect(browserName).toBe('webkit'); expect(hasTouch).toBe(true); expect(isMobile).toBe(true);")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_chromium_can_be_selected_explicitly(self):
        result = self.run_startup(server_output(READY_RECORD),
                                  "expect(browserName).toBe('chromium'); expect(hasTouch).toBe(false);",
                                  browser="chromium")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_unknown_browser_is_rejected_before_startup(self):
        result = self.run_startup(server_output(READY_RECORD), browser="unknown")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("HYZ_THINGS_E2E_BROWSER must be webkit or chromium", result.stdout + result.stderr)

    def test_missing_ready_output_fails_with_startup_timeout(self):
        result = self.run_startup("print('server is initializing', flush=True)")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Timed out waiting 1000ms", result.stdout + result.stderr)
        self.assertNotIn("1 passed", result.stdout)

    def test_another_port_cannot_satisfy_readiness(self):
        record = dict(READY_RECORD, web_origin="http://127.0.0.1:319010")
        result = self.run_startup(server_output(record))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Timed out waiting 1000ms", result.stdout + result.stderr)

    def test_occupied_port_fails_without_reusing_existing_server(self):
        with socket.socket() as occupied:
            occupied.bind(("127.0.0.1", 0))
            occupied.listen()
            result = self.run_startup(
                "import socket\n"
                "listener = socket.socket()\n"
                f"listener.bind({occupied.getsockname()!r})\n"
                + server_output(READY_RECORD)
            )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Address already in use", result.stdout + result.stderr)
        self.assertNotIn("1 passed", result.stdout)


if __name__ == "__main__":
    unittest.main()
