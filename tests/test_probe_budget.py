import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
PATCHER = ROOT / "scripts/patch-probe-budget.py"

class ProbeBudgetTests(unittest.TestCase):
    def test_presence_and_wake_allow_gso_fallback(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            files = {
                "clients/session/src/console.rs": 'let results = trust::probe_known(&hosts, Duration::from_millis(900));\n',
                "crates/pf-client-core/src/orchestrate.rs": 'let online = crate::trust::probe_one(addr, port, fp_hex, Duration::from_millis(900));\n',
            }
            for name, text in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
            result = subprocess.run([sys.executable, str(PATCHER), str(root)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            for name, text in files.items():
                self.assertEqual((root / name).read_text(), text.replace('Duration::from_millis(900)', 'Duration::from_secs(3)'))

    def test_upstream_drift_fails_without_partial_patch(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            console = root / 'clients/session/src/console.rs'
            wake = root / 'crates/pf-client-core/src/orchestrate.rs'
            console.parent.mkdir(parents=True)
            wake.parent.mkdir(parents=True)
            original = 'trust::probe_known(&hosts, Duration::from_millis(900))'
            console.write_text(original)
            wake.write_text('changed upstream')
            result = subprocess.run([sys.executable, str(PATCHER), str(root)], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(console.read_text(), original)

    def test_build_applies_patch_before_cargo(self):
        script = (ROOT / 'scripts/build.sh').read_text()
        self.assertIn('"$ROOT/scripts/patch-probe-budget.py" "$SRC/punktfunk"', script)
        self.assertLess(script.index('patch-probe-budget.py'), script.index('cargo build --locked'))

if __name__ == '__main__':
    unittest.main()
