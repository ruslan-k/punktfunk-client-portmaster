import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class CedarDmabufProbe(unittest.TestCase):
    def test_probe_is_opt_in_read_only_and_leaves_the_frame_path_alone(self):
        module = (ROOT / 'patches/video_cedar.rs').read_text()
        for marker in [
            'PUNKTFUNK_CEDAR_DMABUF_PROBE',
            'fn probe_dmabuf(',
            'libc::PROT_READ',
            'libc::MAP_SHARED',
            'libc::munmap',
            'cedar_dmabuf::matches',
            'self.dmabuf_probe && p.n_buf_fd >= 0',
        ]:
            self.assertIn(marker, module)
        # The probe must not become the frame path: the copy still feeds the frame.
        self.assertIn('CpuPlanarFrame::from_packed_i420(', module)
        self.assertNotIn('CpuPlanarFrame::from_planes(', module)
        self.assertNotIn('libc::PROT_WRITE', module)
        # One probe per descriptor, never per frame.
        self.assertIn('self.probed_fds.contains(&fd)', module)

    def test_helper_ships_through_the_patch_pipeline_and_is_tested(self):
        self.assertTrue((ROOT / 'patches/cedar_dmabuf.rs').is_file())
        phases = (ROOT / 'scripts/patch-cedar-phases.py').read_text()
        self.assertIn('cedar_dmabuf.rs', phases)
        self.assertIn('dmabuf_helper', phases)
        build = (ROOT / 'scripts/build.sh').read_text()
        self.assertIn('crates/pf-client-core/src/cedar_dmabuf.rs', build)
        self.assertIn('rustc --test --edition 2024 "$ROOT/patches/cedar_dmabuf.rs"', build)


if __name__ == '__main__':
    unittest.main()
