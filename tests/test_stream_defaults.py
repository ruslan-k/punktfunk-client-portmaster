from pathlib import Path
import json
import os
import subprocess
import tempfile
import unittest
ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / 'package/punktfunk/stream-defaults.py'

class StreamDefaults(unittest.TestCase):
    def test_first_run_uses_native_720p30_without_decoder_backlog(self):
        self.assertTrue(HELPER.exists())
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / 'client-gtk-settings.json'
            subprocess.run(['python3', str(HELPER), str(p)], check=True)
            d = json.loads(p.read_text())
            self.assertEqual((d['width'], d['height'], d['refresh_hz']), (1280,720,30))
            self.assertEqual(d['codec'], 'h264')

    def test_existing_user_settings_are_never_overwritten(self):
        self.assertTrue(HELPER.exists())
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / 'client-gtk-settings.json'
            original = '{"refresh_hz":60,"unknown":"preserved"}'
            p.write_text(original)
            subprocess.run(['python3', str(HELPER), str(p)], check=True)
            self.assertEqual(p.read_text(), original)
