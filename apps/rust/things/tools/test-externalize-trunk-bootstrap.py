#!/usr/bin/env python3
"""Distribution version covers the entrypoint and every shipped resource."""
import re
import base64
import hashlib
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name('externalize-trunk-bootstrap.py')


class FrontendVersionTests(unittest.TestCase):
    def version(self, resource=b'first', title='Portal', nested=b'nested'):
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            (dist / 'index.html').write_text(
                f'<html><head><title>{title}</title></head><body>'
                '<script type="module">import init from "/app.js"; await init();</script>'
                '</body></html>')
            (dist / 'app.js').write_bytes(resource)
            (dist / 'nested').mkdir()
            (dist / 'nested/index.html').write_bytes(nested)
            subprocess.run(['python3', str(SCRIPT), str(dist)], check=True)
            html = (dist / 'index.html').read_text()
            self.assertNotIn('<script type="module">', html)
            match = re.search(r'<meta name="hyz-frontend-version" content="([a-f0-9]{64})"', html)
            self.assertIsNotNone(match, 'missing distribution version')
            return match.group(1)

    def test_version_is_stable_and_covers_resources_and_html(self):
        original = self.version()
        self.assertEqual(original, self.version())
        self.assertNotEqual(original, self.version(resource=b'changed'))
        self.assertNotEqual(original, self.version(title='Changed'))
        self.assertNotEqual(original, self.version(nested=b'changed'))


    def test_changed_snippets_get_new_import_urls_and_entrypoint(self):
        def build(content):
            with tempfile.TemporaryDirectory() as directory:
                dist = Path(directory)
                original = 'app-0123456789abcdef.js'
                source = "import { value } from './snippets/pkg-0123456789abcdef/ink.js';"
                integrity = base64.b64encode(hashlib.sha384(source.encode()).digest()).decode()
                (dist / 'index.html').write_text(
                    f'<html><head><link rel="modulepreload" href="/{original}" integrity="sha384-{integrity}">'
                    '<link rel="modulepreload" href="/snippets/pkg-0123456789abcdef/ink.js">'
                    '</head><body><script type="module">'
                    f'import init from "/{original}"; await init();</script></body></html>')
                (dist / original).write_text(source)
                snippet = dist / 'snippets/pkg-0123456789abcdef/ink.js'
                snippet.parent.mkdir(parents=True)
                snippet.write_text(content)
                subprocess.run(['python3', str(SCRIPT), str(dist)], check=True)
                bootstrap = (dist / 'router-bootstrap.js').read_text()
                entry = re.search(r'from "/([^"?]+)', bootstrap).group(1)
                source = (dist / entry).read_text()
                imported = re.search(r"from '\./([^']+)'", source).group(1)
                self.assertEqual((dist / imported).read_text(), content)
                html = (dist / 'index.html').read_text()
                updated_integrity = base64.b64encode(hashlib.sha384(source.encode()).digest()).decode()
                self.assertIn(f'href="/{entry}" integrity="sha384-{updated_integrity}"', html)
                self.assertIn(f'href="/{imported}"', html)
                return entry, imported
        first = build('export const value = 1;')
        self.assertEqual(first, build('export const value = 1;'))
        changed = build('export const value = 2;')
        self.assertNotEqual(first[0], changed[0], 'entrypoint still uses an immutable cached URL')
        self.assertNotEqual(first[1], changed[1], 'snippet still uses an immutable cached URL')


class HomeScreenTests(unittest.TestCase):
    def test_ipad_metadata_and_png_icons(self):
        import json
        import struct
        frontend = SCRIPT.parent.parent / 'frontend'
        html = (frontend / 'index.html').read_text()
        self.assertIn('name="apple-mobile-web-app-capable" content="yes"', html)
        self.assertIn('rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png"', html)
        manifest = json.loads((frontend / 'manifest.webmanifest').read_text())
        self.assertEqual(manifest['display'], 'standalone')
        for name, size in [('apple-touch-icon.png', 180), ('pwa-icon-192.png', 192), ('pwa-icon-512.png', 512)]:
            png = (frontend / name).read_bytes()
            self.assertEqual(png[:8], b'\x89PNG\r\n\x1a\n')
            self.assertEqual(struct.unpack('>II', png[16:24]), (size, size))
            self.assertIn(f'href="{name}"', html)
        self.assertTrue(any(icon['type'] == 'image/png' for icon in manifest['icons']))


if __name__ == '__main__':
    unittest.main()
