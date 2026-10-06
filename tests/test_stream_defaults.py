from pathlib import Path
import json
import os
import subprocess
import tempfile
import unittest
ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / 'package/punktfunk/stream-defaults.py'

class StreamDefaults(unittest.TestCase):
    def test_first_run_follows_the_configured_decode_path(self):
        self.assertTrue(HELPER.exists())
        for decoder, expected in [('native-cedar', 60), ('software', 30)]:
            with tempfile.TemporaryDirectory() as tmp:
                p = Path(tmp) / 'client-gtk-settings.json'
                env = dict(os.environ, PUNKTFUNK_DECODER=decoder)
                subprocess.run(['python3', str(HELPER), str(p)], check=True, env=env)
                d = json.loads(p.read_text())
                self.assertEqual((d['width'], d['height'], d['refresh_hz']), (1280, 720, expected))
                self.assertEqual(d['codec'], 'h264')

    def test_existing_user_settings_are_never_overwritten(self):
        self.assertTrue(HELPER.exists())
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / 'client-gtk-settings.json'
            original = '{"refresh_hz":60,"unknown":"preserved"}'
            p.write_text(original)
            subprocess.run(['python3', str(HELPER), str(p)], check=True,
                           env=dict(os.environ, PUNKTFUNK_DECODER='native-cedar'))
            self.assertEqual(p.read_text(), original)
