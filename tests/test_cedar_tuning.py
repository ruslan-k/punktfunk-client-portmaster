from pathlib import Path
import unittest
ROOT=Path(__file__).resolve().parents[1]
class CedarTuningIntegration(unittest.TestCase):
    def test_candidate_axes_are_explicit_logged_and_not_silently_combined(self):
        src=(ROOT/'patches/video_cedar.rs').read_text()
        for marker in ['storage.config.b_no_b_frames = tune.no_b_frames','info.b_is_frame_package = tune.frame_package','storage.config.n_decode_smooth_frame_buffer_num = tune.smooth','storage.config.n_display_holding_frame_buffer_num = tune.display','storage.config.n_ve_freq = tune.ve_freq_mhz.max(0) as c_uint','self.drop_b_delay','cedar: candidate configuration']:
            self.assertIn(marker,src)
        for marker in ['poll_budget_us:','append_aud:','async_parser::retry_async','async_parser::AUD_PTS','fn submit_aud_delimiter','cedar_tuning.rs']:
            self.assertIn(marker,src)
        self.assertIn('CedarTuning::from_lookup',src)
        self.assertIn('PUNKTFUNK_CEDAR_VE_FREQ',(ROOT/'patches/cedar_tuning.rs').read_text())
        build=(ROOT/'scripts/build.sh').read_text()
        self.assertIn('cedar_tuning_tests.rs',build)
        self.assertIn('src/cedar_tuning.rs',build)
        patcher=(ROOT/'scripts/patch-cedar-phases.py').read_text()
        self.assertIn('cedar_tuning.rs',patcher)
