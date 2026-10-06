"""Regression: Spruce's speaker alias must resolve with real ALSA config."""
import ctypes
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / 'package/punktfunk/spruce-alsa-route.py'

class AlsaRoute(unittest.TestCase):
    def test_missing_dmix_group_uses_numeric_process_gid(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp)
            rc = p / '.asoundrc'
            rc.write_text('pcm.spruce_speaker { type asym playback.pcm "Playback" }\n')
            config = p / 'alsa.conf'
            config.write_text('defaults.pcm.ipc_gid "punktfunk_nonexistent_group"\npcm.dmix { type null }\n<' + str(rc) + '>\n')
            subprocess.run(['python3', str(HELPER), str(rc)],
                           env=dict(os.environ, HOME=tmp, ALSA_CONFIG_PATH=str(config)), check=True)
            self.assertIn('defaults.pcm.ipc_gid ' + str(os.getgid()), rc.read_text())

    def test_missing_playback_is_repaired_without_changing_bluetooth_default(self):
        self.assertTrue(HELPER.exists(), 'missing firmware ALSA route repair')
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp)
            rc = p / '.asoundrc'
            original = 'pcm.spruce_speaker { type asym playback.pcm "Playback" }\npcm.spruce_bt { type null }\npcm.!default "spruce_bt"\n'
            rc.write_text(original)
            config = p / 'alsa.conf'
            config.write_text('pcm.dmix { type null }\n<' + str(rc) + '>\n')
            env = dict(os.environ, HOME=tmp, ALSA_CONFIG_PATH=str(config))
            for _ in range(2):
                subprocess.run(['python3', str(HELPER), str(rc)], env=env, check=True)
            fixed = rc.read_text()
            self.assertTrue(fixed.startswith(original))
            self.assertEqual(fixed.count('pcm.Playback {'), 1)
            self.assertIn('slave.pcm "dmix"', fixed)

    def test_existing_firmware_playback_is_untouched(self):
        self.assertTrue(HELPER.exists(), 'missing firmware ALSA route repair')
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp)
            rc = p / '.asoundrc'
            original = 'pcm.spruce_speaker { type asym playback.pcm "Playback" }\n'
            rc.write_text(original)
            config = p / 'alsa.conf'
            config.write_text('pcm.Playback { type null }\n<' + str(rc) + '>\n')
            subprocess.run(['python3', str(HELPER), str(rc)],
                           env=dict(os.environ, HOME=tmp, ALSA_CONFIG_PATH=str(config)), check=True)
            self.assertEqual(rc.read_text(), original)
