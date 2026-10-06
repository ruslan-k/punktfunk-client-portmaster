"""Run real shell launchers with isolated PortMaster/native-binary fixtures."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class Launchers(unittest.TestCase):
    def run_launcher(self, name, host="none"):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ports = root / "ports"
            shutil.copytree(ROOT / "package", ports)
            game = ports / "punktfunk"
            pm = root / "pm"
            pm.mkdir()
            (pm / "control.txt").write_text(
                'directory="' + str(root) + '"\nget_controls() { :; }\n'
            )
            (pm / "device_info.txt").write_text("CFW_NAME=test\n")
            bins = game / "bin"
            bins.mkdir()
            cli = bins / "punktfunk"
            cli.write_text('#!/bin/bash\nif [ "$1" = default-host ]; then printf "%s\\n" "$TEST_HOST"; else exit 99; fi\n')
            session = bins / "punktfunk-session"
            session.write_text('#!/usr/bin/env python3\nimport os,json,sys\nopen(os.environ["TEST_ARGS"],"w").write(json.dumps({"argv":sys.argv[1:],"video":os.environ.get("SDL_VIDEO_DRIVER"),"fontconfig_ok":os.path.isfile(os.environ.get("FONTCONFIG_FILE", ""))}))\nsys.exit(23)\n')
            cli.chmod(0o755)
            session.chmod(0o755)
            args = root / "args.json"
            env = dict(os.environ, PUNKTFUNK_CONTROL_FOLDER=str(pm),
                       TEST_HOST=host, TEST_ARGS=str(args), HOME=str(root))
            env.pop("DISPLAY", None)
            env.pop("WAYLAND_DISPLAY", None)
            run = subprocess.run(["bash", str(ports / name)], env=env,
                                 capture_output=True, text=True, timeout=10)
            self.assertTrue(args.exists(), run.stdout + run.stderr)
            self.assertEqual(run.returncode, 23, run.stdout + run.stderr)
            log = "\n".join(f.read_text() for f in (game / "logs").glob("*.log"))
            self.assertIn("session_exit=23", log)
            return json.loads(args.read_text())

    def test_package_has_only_one_menu_launcher(self):
        import xml.etree.ElementTree as ET
        package = ROOT / "package"
        self.assertEqual(sorted(p.name for p in package.glob("*.sh")), ["Punktfunk.sh"])
        metadata = json.loads((package / "port.json").read_text())
        self.assertEqual(metadata["items"], ["Punktfunk.sh", "punktfunk"])
        paths = [g.findtext("path") for g in ET.parse(package / "gameinfo.xml").getroot()]
        self.assertEqual(paths, ["./Punktfunk.sh"])

    def test_main_without_host_opens_host_console(self):
        result = self.run_launcher("Punktfunk.sh")
        self.assertEqual(result["argv"], ["--browse", "--fullscreen"])

    def test_main_with_host_opens_library(self):
        result = self.run_launcher("Punktfunk.sh", "saved\t192.0.2.10:47990")
        self.assertEqual(result["argv"], ["--browse", "192.0.2.10:47990", "--fullscreen", "--stats"])

    def test_build_enables_session_console_ui(self):
        build = (ROOT / "scripts/build.sh").read_text()
        self.assertIn("--features punktfunk-client-session/ui", build)

    def test_launcher_uses_port_local_fontconfig(self):
        self.assertTrue(self.run_launcher("Punktfunk.sh")["fontconfig_ok"])

    def test_no_desktop_display_selects_kmsdrm(self):
        self.assertEqual(self.run_launcher("Punktfunk.sh")["video"], "kmsdrm")


if __name__ == "__main__":
    unittest.main()
