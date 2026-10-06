import pathlib,unittest
ROOT=pathlib.Path(__file__).resolve().parents[1]
class PtsIntegrationTests(unittest.TestCase):
    def test_token_probe_is_opt_in_and_counts_exact_matches(self):
        src=(ROOT/'patches/video_cedar.rs').read_text()
        for marker in ['PUNKTFUNK_CEDAR_PTS_PROBE','cedar-pts-submit','cedar-pts-output','probe.take(p.n_pts)']:
            self.assertIn(marker,src)
    def test_build_exercises_pts_ledger_and_ships_helper(self):
        src=(ROOT/'scripts/build.sh').read_text()
        self.assertIn('tests/cedar_pts_tests.rs',src)
        src=(ROOT/'scripts/patch-cedar-phases.py').read_text()
        self.assertIn('cedar_pts.rs',src)
if __name__=='__main__':unittest.main()
