import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'scripts/patch-presenter-refuse-import.py'


def load():
    spec = importlib.util.spec_from_file_location('refuse_patch', SCRIPT)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PresenterRefuseImport(unittest.TestCase):
    def test_the_hook_takes_the_presenters_own_refusal_path(self):
        """A working import must still be able to reach the pump's fallback.

        `force_software.store(true)` plus `Ok(false)` is what the presenter already
        does when it has no import support, so the hook drives the real signal.
        """
        module = load()
        applied = dict((label, new) for label, old, new in module.EDITS)
        arm = applied['refuse hook in the dmabuf arm']
        self.assertIn('if refuse_import() {', arm)
        self.assertIn('self.force_software.store(true, Ordering::Relaxed);', arm)
        self.assertIn('return Ok(false);', arm)
        self.assertLess(arm.index('if refuse_import() {'),
                        arm.index('// No import extensions'))
        helper = applied['the hook helper']
        self.assertIn('fn refuse_import() -> bool', helper)
        self.assertIn('PUNKTFUNK_CEDAR_REFUSE_IMPORT', helper)
        self.assertIn('OnceLock', helper)

    def test_it_applies_once_and_refuses_drift(self):
        module = load()
        seed = '\n'.join(old for _, old, _ in module.EDITS)
        for drop in [None] + [label for label, _, _ in module.EDITS]:
            with tempfile.TemporaryDirectory() as tmp:
                path = module.target_for(pathlib.Path(tmp))
                path.parent.mkdir(parents=True)
                text = seed
                if drop is not None:
                    old = dict((label, old) for label, old, _ in module.EDITS)[drop]
                    text = text.replace(old, 'ANCHOR DRIFT')
                path.write_text(text)
                result = subprocess.run([sys.executable, str(SCRIPT), tmp],
                                        capture_output=True, text=True)
                if drop is not None:
                    self.assertNotEqual(result.returncode, 0, drop)
                    self.assertEqual(path.read_text(), text)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(path.read_text().count('fn refuse_import() -> bool'), 1)
                    self.assertEqual(path.read_text().count('if refuse_import() {'), 1)

    def test_the_hook_ships_in_the_presenter_build(self):
        build = (ROOT / 'scripts/build.sh').read_text()
        self.assertLess(build.index('patch-presenter-planar.py'),
                        build.index('patch-presenter-refuse-import.py'))
        self.assertLess(build.index('patch-presenter-refuse-import.py'),
                        build.index('cargo build --locked'))
