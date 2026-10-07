import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]

class CedarPhaseTests(unittest.TestCase):
    def test_profile_is_opt_in_and_observes_vendor_return_codes(self):
        src = (ROOT / 'patches/video_cedar.rs').read_text()
        self.assertIn('PUNKTFUNK_CEDAR_PROFILE', src)
        self.assertIn('profile.note_vendor(rc,', src)
        self.assertIn('profile.note_copy(', src)
        self.assertIn('cedar-phase-json', src)
        # Phase counters preserve drain semantics; separate PTS patch names each AU.
        self.assertIn('self.pts_clock.next(capture_ns)', src)
        self.assertIn('VDECODE_RESULT_CONTINUE | VDECODE_RESULT_NO_BITSTREAM => {', src)
        self.assertIn('async_parser::retry_async(rc, *produced, elapsed_us(self.async_start), self.poll_budget_us)', src)
        # The drain body is a separate method returning its control decision, so
        # `continue`/`break` cannot skip its own timer.
        self.assertIn('fn drain_body(', src)
        self.assertIn('DrainStep::Again', src)
        self.assertIn('profile.note_stage(6, elapsed_us(body_begin))', src)
        self.assertIn('profile.note_stage(7, elapsed_us(drain_begin))', src)
        # The retry backoff sits inside the drain body but outside the arm timers:
        # without its own column a measured 0.26 ms/frame read as unattributed.
        self.assertIn('profile.note_stage(10, elapsed_us(sleep_begin))', src)
        self.assertIn('Duration::from_micros(self.retry_us)', src)
        phases = (ROOT / 'patches/cedar_phases.rs').read_text()
        self.assertIn('pub stage_n: [u64; 11],', phases)
        self.assertIn('10 is the', phases)

    def test_fifo_default_has_explicit_baseline_control_and_is_bounded(self):
        src = (ROOT / 'patches/video_cedar.rs').read_text()
        self.assertIn('PUNKTFUNK_CEDAR_FIFO', src)
        self.assertIn('as_deref() != Ok("0")', src)
        self.assertIn('queue_picture(', src)
        self.assertIn('pending.len() < 32', src)

    def test_session_patcher_rejects_each_drift_before_copying_helper(self):
        import importlib.util
        import tempfile
        import subprocess
        import sys
        script = ROOT / 'scripts/patch-cedar-phases.py'
        spec = importlib.util.spec_from_file_location('cedar_phases_patch', script)
        assert spec is not None and spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        for bad in [None, module.START, module.DECODE]:
            with tempfile.TemporaryDirectory() as temp:
                src = pathlib.Path(temp) / 'crates/pf-client-core/src'
                src.mkdir(parents=True)
                text = module.START + module.DECODE
                if bad is not None:
                    text = text.replace(bad, '')
                path = src / 'session.rs'
                path.write_text(text)
                result = subprocess.run([sys.executable, str(script), temp, str(ROOT)],
                                        capture_output=True, text=True)
                if bad is None:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn('queue_us', path.read_text())
                    self.assertTrue((src / 'cedar_phases.rs').is_file())
                else:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(path.read_text(), text)
                    self.assertFalse((src / 'cedar_phases.rs').exists())

    def test_profile_helper_has_executable_rust_tests_in_build(self):
        self.assertTrue((ROOT / 'patches/cedar_phases.rs').is_file())
        script = (ROOT / 'scripts/build.sh').read_text()
        self.assertIn('rustc --test', script)
        self.assertIn('cedar-phase-tests', script)
        self.assertIn('patch-cedar-phases.py', script)
        self.assertLess(script.index('patch-cedar-phases.py'), script.index('cargo build --locked'))

if __name__ == '__main__':
    unittest.main()
