import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class CedarFastAu(unittest.TestCase):
    def test_the_cheap_path_is_wired_and_can_be_pinned_off(self):
        module = (ROOT / 'patches/video_cedar.rs').read_text()
        for marker in [
            '#[path = "cedar_fast_au.rs"]',
            'mod fast_au;',
            'fast_au::classify(au, &bits)',
            'fast_au::AuClass::Plain',
            'fn plain_facts(',
            'fn plan_facts_full(',
            'self.plan_facts_full(au)',
            'PUNKTFUNK_CEDAR_FAST_PLAN',
            'PUNKTFUNK_CEDAR_FAST_VERIFY',
        ]:
            self.assertIn(marker, module)
        # The dispatcher must not call itself: the cheap path delegates to the
        # full planner, never back to the dispatcher.
        dispatcher = module[module.index('fn plan_facts(&mut self'):]
        dispatcher = dispatcher[:dispatcher.index('fn plan_facts_full(')]
        self.assertNotIn('self.plan_facts(au)', dispatcher)

    def test_the_full_planner_stays_authoritative_and_refreshes_the_cache(self):
        module = (ROOT / 'patches/video_cedar.rs').read_text()
        full = module[module.index('fn plan_facts_full(&mut self'):]
        # Whatever the cheap path needs must come from the real plan.
        for marker in [
            'log2_max_frame_num_minus4: plan.sps.log2_max_frame_num_minus4',
            'separate_colour_plane: plan.sps.separate_colour_plane_flag',
            'self.poc_type = plan.sps.pic_order_cnt_type',
            'self.frame_mbs_only = plan.sps.frame_mbs_only_flag',
            'self.last_plan_truth = Some(PlanTruth {',
        ]:
            self.assertIn(marker, full)

    def test_the_scanner_refuses_everything_that_carries_state(self):
        scanner = (ROOT / 'patches/cedar_fast_au.rs').read_text()
        for marker in [
            'NAL_IDR | NAL_SEI | NAL_SPS | NAL_PPS => return AuClass::NeedsFullPlan',
            'Some(_) => return AuClass::NeedsFullPlan',
            'None => return AuClass::NeedsFullPlan',
            '(true, Some(num)) => AuClass::Plain',
            '_ => AuClass::NeedsFullPlan',
            'slice_type % 5 == 1',
            "b == 0x03",
        ]:
            self.assertIn(marker, scanner)
        # A wrong "plain" answer would arm the hardware gate on a reordered
        # stream, so the B-slice and IDR cases must be covered by unit tests.
        for test in ['a_b_slice_is_reported_so_the_gate_can_refuse',
                     'state_carrying_nals_always_ask_for_the_full_planner',
                     'emulation_prevention_inside_the_header_is_stripped',
                     'a_truncated_header_is_refused_rather_than_guessed']:
            self.assertIn(test, scanner)

    def test_verification_uses_the_oracle_and_counts_disagreements(self):
        module = (ROOT / 'patches/video_cedar.rs').read_text()
        verify = module[module.index('fn verify_plain_au('):]
        verify = verify[:verify.index('fn plan_facts(')]
        self.assertIn('let planned = self.plan_facts_full(au);', verify)
        self.assertIn('self.fast_mismatches += 1;', verify)
        self.assertIn('"cedar-fast-au: the cheap scanner disagreed with the full planner"', verify)
        # The oracle's answer is what ships in verification mode.
        self.assertTrue(verify.rstrip().endswith('planned\n    }') or 'planned' in verify.split('fn ')[0])

    def test_helper_ships_through_the_patch_pipeline_and_is_tested(self):
        self.assertTrue((ROOT / 'patches/cedar_fast_au.rs').is_file())
        phases = (ROOT / 'scripts/patch-cedar-phases.py').read_text()
        self.assertIn('cedar_fast_au.rs', phases)
        self.assertIn('fast_au_helper', phases)
        build = (ROOT / 'scripts/build.sh').read_text()
        self.assertIn('crates/pf-client-core/src/cedar_fast_au.rs', build)
        self.assertIn('rustc --test --edition 2024 "$ROOT/patches/cedar_fast_au.rs"', build)


if __name__ == '__main__':
    unittest.main()
