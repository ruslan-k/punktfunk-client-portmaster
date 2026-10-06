import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class CedarReleaseGuard(unittest.TestCase):
    """The guard is what makes zero-copy safe, so it is pinned before the import.

    The presenter-side three-plane import is NOT here yet: a first attempt bolted a
    separate import beside `get_or_import`, which takes the frame by value and
    caches imported images per `pool_key`. That attempt would have re-created three
    Vulkan images per frame instead of per picture slot - more expensive than the
    copy it removes - so it was reverted rather than shipped. The real change
    generalises `HwFrame`/`Planes` to a plane list plus a `planar` flag and keeps
    the cache.
    """

    def test_the_release_guard_sends_the_token_and_is_sendable(self):
        module = (ROOT / 'patches/video_cedar.rs').read_text()
        for marker in [
            'pub(crate) struct CedarFrameGuard {',
            'release: std::sync::mpsc::Sender<u64>,',
            'impl Drop for CedarFrameGuard {',
            'let _ = self.release.send(self.token);',
        ]:
            self.assertIn(marker, module)
        # The vendor stack is single-threaded: the guard only hands the token back,
        # it never calls into the vendor itself.
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


if __name__ == '__main__':
    unittest.main()
