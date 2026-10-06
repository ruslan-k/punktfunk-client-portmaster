import pathlib,unittest
ROOT=pathlib.Path(__file__).resolve().parents[1]
class PtsIntegrationTests(unittest.TestCase):
    def test_token_probe_is_opt_in_and_counts_exact_matches(self):
        src=(ROOT/'patches/video_cedar.rs').read_text()
        for marker in ['PUNKTFUNK_CEDAR_PTS_PROBE','cedar-pts-submit','cedar-pts-output','self.pts_ledger.take(p.n_pts)']:
            self.assertIn(marker,src)
    def test_exact_output_metadata_pipeline_is_installed(self):
        self.assertTrue((ROOT/'scripts/patch-cedar-pts.py').exists())
        src=(ROOT/'patches/video_cedar.rs').read_text()
        for marker in ['source.facts', 'source.color', 'output_stamp = output.stamp', 'unmatched output PTS; refusing FIFO-order guess']:
            self.assertIn(marker,src)

    def test_pts_patcher_transforms_both_files_and_drift_writes_nothing(self):
        import tempfile,subprocess,sys,importlib.util
        script=ROOT/'scripts/patch-cedar-pts.py'
        spec=importlib.util.spec_from_file_location('pts_patch',script)
        assert spec and spec.loader
        module=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        originals={name: '\n'.join(old for old,new in edits) for name,edits in
                   [('video.rs',module.VIDEO_EDITS),('session.rs',module.SESSION_EDITS)]}
        cases=[None]+[(name,old) for name,edits in [('video.rs',module.VIDEO_EDITS),('session.rs',module.SESSION_EDITS)] for old,new in edits]
        for case in cases:
            with tempfile.TemporaryDirectory() as temp:
                src=pathlib.Path(temp)/'crates/pf-client-core/src'
                src.mkdir(parents=True)
                seed=originals.copy()
                if case:
                    name,old=case
                    seed[name]=seed[name].replace(old,'ANCHOR DRIFT')
                for name,text in seed.items():(src/name).write_text(text)
                result=subprocess.run([sys.executable,str(script),temp],capture_output=True,text=True)
                if case:
                    self.assertNotEqual(result.returncode,0)
                    self.assertEqual({name:(src/name).read_text() for name in seed},seed)
                else:
                    self.assertEqual(result.returncode,0,result.stderr)
                    text=(src/'session.rs').read_text()
                    self.assertIn('pts_ns: output_pts_ns',text)
                    self.assertIn('source', (ROOT/'patches/video_cedar.rs').read_text())
                    self.assertIn('output_flags',text)
                    self.assertIn('p.pixels_ready_ns.unwrap_or_else(now_ns)',text)
                    self.assertIn('refusing current-AU guess',text)
        build=(ROOT/'scripts/build.sh').read_text()
        self.assertLess(build.index('patch-cedar-phases.py'),build.index('patch-cedar-pts.py'))
        self.assertLess(build.index('patch-cedar-pts.py'),build.index('cargo build --locked'))

    def test_build_exercises_pts_ledger_and_ships_helper(self):
        src=(ROOT/'scripts/build.sh').read_text()
        self.assertIn('tests/cedar_pts_tests.rs',src)
        src=(ROOT/'scripts/patch-cedar-phases.py').read_text()
        self.assertIn('cedar_pts.rs',src)
if __name__=='__main__':unittest.main()
