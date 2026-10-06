import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
UPSTREAM = pathlib.Path('/var/home/ruslan/.hermes/cache/scratch/punktfunk-upstream')
SCRIPT = ROOT / 'scripts/patch-presenter-planar.py'


class PresenterPlanarImport(unittest.TestCase):
    """The presenter half of zero-copy, and the guard the client half needs.

    The three-plane import has to reuse the existing cached importer: `get_or_import`
    takes the frame by value and caches imported images per `pool_key`, and
    `let p = &cache.planes[..]` hands out a reference, so the plane arrays must stay
    `Copy` (fixed `MAX_PLANES`, never `Vec`). An earlier attempt bolted a separate
    import beside it, which would have re-created three images every frame - more
    expensive than the 0.9 ms copy it removes - and was reverted.
    """

    def test_the_patch_generalises_planes_and_adds_the_planar_path(self):
        script = SCRIPT.read_text()
        for marker in [
            'const MAX_PLANES: usize = 3;',
            'const DRM_FORMAT_YUV420: u32 = 0x3231_5559;',
            'const DRM_FORMAT_YVU420: u32 = 0x3231_5659;',
            'pub views: [vk::ImageView; MAX_PLANES],',
            'pub fn plane_images(&self) -> &[vk::Image]',
            'layout: [(u32, u32); MAX_PLANES],',
            'planes: wanted as u8,',
            'self.csc_planar.bind_planes_planar(',
            'for &view_image in f.plane_images()',
        ]:
            self.assertIn(marker, script)
        # The interleaved formats keep their own path and their own formats.
        for kept in [
            'DRM_FORMAT_NV12 => (vk::Format::R8_UNORM, vk::Format::R8G8_UNORM, false)',
            'DRM_FORMAT_P010 => (vk::Format::R16_UNORM, vk::Format::R16G16_UNORM, false)',
        ]:
            self.assertIn(kept, script)
        # The signature keeps the call site's parameter order.
        self.assertIn(
            'fn import(\n    instance: &ash::Instance,\n    pdev: vk::PhysicalDevice,\n'
            '    device: &ash::Device,',
            script,
        )

    def test_the_guard_returns_the_picture_only_after_the_presenter_drops_it(self):
        module = (ROOT / 'patches/video_cedar.rs').read_text()
        for marker in [
            'pub(crate) struct CedarFrameGuard {',
            'release: std::sync::mpsc::Sender<u64>,',
            'impl Drop for CedarFrameGuard {',
            'let _ = self.release.send(self.token);',
        ]:
            self.assertIn(marker, module)
        # The vendor stack is single-threaded: the guard only hands the token back.
        guard = module[module.index('impl Drop for CedarFrameGuard'):]
        guard = guard[:guard.index('\n}\n')]
        self.assertNotIn('return_picture', guard)
        self.assertNotIn('self.libs', guard)

    def test_the_frame_guard_enum_gains_the_cedar_arm(self):
        patcher = (ROOT / 'scripts/patch-cedar.py').read_text()
        for marker in ['GUARD_OLD', 'GUARD_NEW',
                       'Cedar(crate::video_cedar::CedarFrameGuard)',
                       '("FrameGuard enum arm", GUARD_OLD, GUARD_NEW)']:
            self.assertIn(marker, patcher)

    def test_a_zero_copy_frame_reports_silence_not_a_damaged_chain(self):
        """`references_clean: false` on a dmabuf frame is EVIDENCE, not silence.

        `anchor_evidence()` maps a dma-buf frame's `references_clean` to
        `ReferencesClean`/`ReferencesDamaged`, and the reanchor gate withholds the
        anchor on `ReferencesDamaged`. This rung has no local parser for the
        reference chain (`LocalRecovery` carries only SEI flags), so a zero-copy
        frame must answer `Unavailable` like the CPU arm does - otherwise frames
        decode and nothing is ever presented. Measured on device: 3900 frames
        decoded, screen still on the connection page, no errors logged.
        """
        patcher = (ROOT / 'scripts/patch-cedar.py').read_text()
        self.assertIn('ANCHOR_OLD', patcher)
        self.assertIn('crate::video_cedar::DECODER_PIN', patcher)
        self.assertIn('return AnchorEvidence::Unavailable;', patcher)
        self.assertIn('("anchor evidence for this rung", ANCHOR_OLD, ANCHOR_NEW)', patcher)
        module = (ROOT / 'patches/video_cedar.rs').read_text()
        self.assertIn('a damaged\n                // chain makes the reanchor gate withhold every anchor.', module)

    def test_the_build_applies_the_presenter_patch(self):
        self.assertIn('patch-presenter-planar.py', (ROOT / 'scripts/build.sh').read_text())

    def test_it_applies_to_the_pinned_sources_and_refuses_drift(self):
        target = UPSTREAM / 'crates/pf-presenter/src/dmabuf.rs'
        if not target.is_file():
            self.skipTest('pinned upstream checkout not present')
        # The pristine text comes from git, not the working tree: the checkout is also
        # used to try patches by hand, so its state must not decide this test.
        def pristine(path):
            out = subprocess.run(
                ['git', '-C', str(UPSTREAM), 'show', f'HEAD:{path}'],
                capture_output=True, text=True,
            )
            self.assertEqual(out.returncode, 0, out.stderr)
            return out.stdout

        original = pristine('crates/pf-presenter/src/dmabuf.rs')
        original_present = pristine('crates/pf-presenter/src/vk/present.rs')
        self.assertNotIn('MAX_PLANES', original)
        with tempfile.TemporaryDirectory() as temp:
            # A tree whose anchors are intact applies, and re-running is a no-op.
            patched = pathlib.Path(temp)
            (patched / 'crates/pf-presenter/src/vk').mkdir(parents=True)
            (patched / 'crates/pf-presenter/src/dmabuf.rs').write_text(original)
            (patched / 'crates/pf-presenter/src/vk/present.rs').write_text(original_present)
            first = subprocess.run([sys.executable, str(SCRIPT), temp],
                                   capture_output=True, text=True)
            self.assertEqual(first.returncode, 0, first.stdout + first.stderr)
            text = (patched / 'crates/pf-presenter/src/dmabuf.rs').read_text()
            self.assertIn('const MAX_PLANES: usize = 3;', text)
            self.assertIn('fn import(', text)
            self.assertNotIn('fn import(\n    device: &ash::Device,', text)
            again = subprocess.run([sys.executable, str(SCRIPT), temp],
                                   capture_output=True, text=True)
            self.assertEqual(again.returncode, 0, again.stdout + again.stderr)
            self.assertEqual((patched / 'crates/pf-presenter/src/dmabuf.rs').read_text(), text)
        # Drift writes nothing at all.
        with tempfile.TemporaryDirectory() as temp:
            drifted = pathlib.Path(temp)
            (drifted / 'crates/pf-presenter/src/vk').mkdir(parents=True)
            (drifted / 'crates/pf-presenter/src/dmabuf.rs').write_text('drifted\n')
            (drifted / 'crates/pf-presenter/src/vk/present.rs').write_text('drifted\n')
            bad = subprocess.run([sys.executable, str(SCRIPT), temp],
                                 capture_output=True, text=True)
            self.assertEqual(bad.returncode, 1)
            # SystemExit's message lands on stderr; either stream is fine, silence is not.
            self.assertIn('drifted', bad.stdout + bad.stderr)
            self.assertEqual((drifted / 'crates/pf-presenter/src/dmabuf.rs').read_text(),
                             'drifted\n')


if __name__ == '__main__':
    unittest.main()
