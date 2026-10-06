import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
# The pinned upstream checkout the port patches, when it is present locally.
UPSTREAM = pathlib.Path('/var/home/ruslan/.hermes/cache/scratch/punktfunk-upstream')


class CedarZeroCopy(unittest.TestCase):
    def test_the_release_guard_sends_the_token_and_is_sendable(self):
        module = (ROOT / 'patches/video_cedar.rs').read_text()
        for marker in [
            'pub(crate) struct CedarFrameGuard {',
            'release: std::sync::mpsc::Sender<u64>,',
            'impl Drop for CedarFrameGuard {',
            'let _ = self.release.send(self.token);',
        ]:
            self.assertIn(marker, module)
        # The vendor stack is single-threaded: the guard must only hand the token
        # back, never call into the vendor itself.
        guard = module[module.index('impl Drop for CedarFrameGuard'):]
        guard = guard[:guard.index('\n}\n')]
        self.assertNotIn('return_picture', guard)
        self.assertNotIn('self.libs', guard)
        for test in ['the_release_guard_sends_its_token_when_the_presenter_drops_it',
                     'the_release_guard_can_travel_to_the_presenter_thread']:
            self.assertIn(test, module)

    def test_the_frame_guard_enum_gains_the_cedar_arm(self):
        patcher = (ROOT / 'scripts/patch-cedar.py').read_text()
        self.assertIn('GUARD_OLD', patcher)
        self.assertIn('GUARD_NEW', patcher)
        self.assertIn('Cedar(crate::video_cedar::CedarFrameGuard)', patcher)
        self.assertIn('("FrameGuard enum arm", GUARD_OLD, GUARD_NEW)', patcher)

    def test_the_presenter_imports_three_planar_planes(self):
        script = (ROOT / 'scripts/patch-presenter-planar.py').read_text()
        for marker in [
            'pub struct HwFramePlanar {',
            'views: [vk::ImageView; 3],',
            'fn import_planar(',
            "frame.planes.len() != 3",
            'vk::Format::R8_UNORM',
            'DrmFrameGuard(FrameGuard::Cedar)',
            'plane_image(',
        ]:
            self.assertIn(marker, script)
        # Two-plane formats must keep their own path: the new import is additive.
        self.assertNotIn('DRM_FORMAT_NV12 =>', script)
        self.assertNotIn('import(NV12', script)

    def test_the_presenter_anchor_matches_the_pinned_revision(self):
        script = (ROOT / 'scripts/patch-presenter-planar.py').read_text()
        anchor = script[script.index("ANCHOR = '''") + len("ANCHOR = '''"):]
        anchor = anchor[:anchor.index("'''")]
        target = UPSTREAM / 'crates/pf-presenter/src/dmabuf.rs'
        if not target.is_file():
            self.skipTest('pinned upstream checkout not present')
        self.assertIn(anchor, target.read_text())

    def test_the_build_applies_the_presenter_patch(self):
        build = (ROOT / 'scripts/build.sh').read_text()
        self.assertIn('patch-presenter-planar.py', build)


if __name__ == '__main__':
    unittest.main()
