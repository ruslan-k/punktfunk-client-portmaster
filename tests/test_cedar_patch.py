import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
PATCHER = ROOT / "scripts/patch-cedar.py"

# The eight anchors live in the pinned upstream revision
# (sources.env PUNKTFUNK_REF) and are asserted unique by the patcher itself.
LIB_ANCHOR = '#[cfg(all(desktop, target_os = "linux"))]\npub mod video_vaapi_native;\n'
ENUM_ANCHOR = '    NativeV4l2(Box<crate::video_v4l2::NativeV4l2Decoder>),\n'
TRIED_ANCHOR = '        let mut native_tried = false;\n'
DECODE_ANCHOR = '            Backend::NativeV4l2(d) => (d.decode(au), d.take_recovery_request()),\n'
WHICH_ANCHOR = '                    Backend::NativeV4l2(_) => "native V4L2",\n'
LOGRUNG_ANCHOR = (
    '        Backend::NativeV4l2(_) => (\n'
    '            NativeRung::V4l2.name(),\n'
    '            Some(native_evidence(NativeRung::V4l2, wire)),\n'
    '        ),\n'
)
PACKED_ANCHOR = '    /// Take three already tight planes. Refuses a plane whose length is not\n'
GUARD_ANCHOR = (
    'pub(crate) enum FrameGuard {\n'
    '    Va(crate::video_vaapi_native::VaFrameGuard),\n'
    '    V4l2(crate::video_v4l2::V4l2FrameGuard),\n'
    '}\n'
)


def fixture_tree(root: pathlib.Path) -> None:
    src = root / "crates/pf-client-core/src"
    src.mkdir(parents=True)
    (src / "lib.rs").write_text("prefix\n" + LIB_ANCHOR + "suffix\n")
    (src / "video.rs").write_text(
        "prefix\n"
        + ENUM_ANCHOR + "mid1\n"
        + TRIED_ANCHOR + "mid2\n"
        + DECODE_ANCHOR + "mid3\n"
        + WHICH_ANCHOR + "mid4\n"
        + LOGRUNG_ANCHOR + "mid5\n"
        + PACKED_ANCHOR + "mid6\n"
        + GUARD_ANCHOR + "suffix\n"
    )


class CedarPatchTests(unittest.TestCase):
    def run_patcher(self, root: pathlib.Path):
        return subprocess.run(
            [sys.executable, str(PATCHER), str(root), str(ROOT)],
            capture_output=True,
            text=True,
        )

    def test_installs_module_and_wires_every_anchor(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            fixture_tree(root)
            result = self.run_patcher(root)
            self.assertEqual(result.returncode, 0, result.stderr)
            src = root / "crates/pf-client-core/src"
            self.assertEqual(
                (src / "video_cedar.rs").read_text(),
                (ROOT / "patches/video_cedar.rs").read_text(),
            )
            self.assertIn("mod video_cedar;", (src / "lib.rs").read_text())
            video = (src / "video.rs").read_text()
            for marker in [
                "NativeCedar(Box<crate::video_cedar::NativeCedarDecoder>),",
                "crate::video_cedar::DECODER_PIN",
                "Backend::NativeCedar(c) => (",
                'Backend::NativeCedar(_) => "native Cedar",',
                '("native-cedar", None),',
                "pub(crate) fn from_packed_i420(",
            ]:
                self.assertIn(marker, video)
            # The anchors are the pinned text still present exactly once each —
            # the patch extends them instead of rewriting them.
            for anchor in [ENUM_ANCHOR, TRIED_ANCHOR, DECODE_ANCHOR, WHICH_ANCHOR, LOGRUNG_ANCHOR, PACKED_ANCHOR]:
                self.assertEqual(video.count(anchor), 1, anchor)

    def test_drift_on_any_anchor_fails_without_writing_anything(self):
        anchors = [LIB_ANCHOR, ENUM_ANCHOR, TRIED_ANCHOR, DECODE_ANCHOR, WHICH_ANCHOR, LOGRUNG_ANCHOR, PACKED_ANCHOR]
        for anchor in anchors:
            with tempfile.TemporaryDirectory() as temp:
                root = pathlib.Path(temp)
                fixture_tree(root)
                for name in ["lib.rs", "video.rs"]:
                    path = root / "crates/pf-client-core/src" / name
                    text = path.read_text()
                    if anchor in text:
                        # Whitespace drift: the one change upstream never promises to keep.
                        path.write_text(text.replace(anchor, anchor[:-1] + " \n", 1))
                before = {
                    n: (root / "crates/pf-client-core/src" / n).read_text()
                    for n in ["lib.rs", "video.rs"]
                }
                result = self.run_patcher(root)
                self.assertNotEqual(result.returncode, 0, f"drift must fail: {anchor!r}")
                after = {
                    n: (root / "crates/pf-client-core/src" / n).read_text()
                    for n in ["lib.rs", "video.rs"]
                }
                self.assertEqual(before, after, f"no partial write for {anchor!r}")
                self.assertFalse(
                    (root / "crates/pf-client-core/src/video_cedar.rs").exists(),
                    f"no module copy for {anchor!r}",
                )

    def test_build_applies_patch_and_formats_the_module(self):
        script = (ROOT / "scripts/build.sh").read_text()
        self.assertIn('"$ROOT/scripts/patch-cedar.py" "$SRC/punktfunk" "$ROOT"', script)
        self.assertLess(
            script.index("patch-cedar.py"),
            script.index("cargo build --locked"),
            "the patch must land before the compile that proves it",
        )
        self.assertIn("crates/pf-client-core/src/video_cedar.rs", script)

    def test_package_defaults_to_the_cedar_pin_with_validation(self):
        config = (ROOT / "package/punktfunk/config.env").read_text()
        self.assertIn("PUNKTFUNK_DECODER=native-cedar", config)
        runtime = (ROOT / "package/punktfunk/runtime-env.sh").read_text()
        self.assertIn("${PUNKTFUNK_DECODER:-native-cedar}", runtime)
        validate = (ROOT / "scripts/validate-package.sh").read_text()
        self.assertIn("native-cedar", validate)

    def test_module_carries_the_abi_and_logging_pins(self):
        module = (ROOT / "patches/video_cedar.rs").read_text()
        for marker in [
            'pub(crate) const DECODER_PIN: &str = "native-cedar";',
            "PIXEL_FORMAT_YUV_PLANER_420",
            "RequestVideoStreamBuffer",
            "SubmitVideoStreamData",
            "DecodeVideoStream",
            "ReturnPicture",
            'target: "cedar"',
            "dlopen",
        ]:
            self.assertIn(marker, module)

    def test_module_uses_one_final_i420_allocation(self):
        module = (ROOT / "patches/video_cedar.rs").read_text()
        self.assertIn("Vec::<u8>::with_capacity(total_len)", module)
        self.assertIn("copy_plane_into(", module)
        self.assertIn("CpuPlanarFrame::from_packed_i420(", module)
        self.assertNotIn("CpuPlanarFrame::from_planes(", module)
        self.assertNotIn("fn copy_plane(", module)
        self.assertIn("stride == w", module)

    def test_module_feeds_stream_packages_like_the_vendor_demo(self):
        # Default still follows vdecoderDemo; diagnostic overrides are explicit.
        # Earlier negative frame-package evidence predated the corrected ABI.
        module = (ROOT / "patches/video_cedar.rs").read_text()
        self.assertIn("info.b_is_frame_package = tune.frame_package;", module)
        tuning = (ROOT / "patches/cedar_tuning.rs").read_text()
        self.assertIn('get("PUNKTFUNK_CEDAR_FRAME_PACKAGE"),0,0,1', tuning)
        self.assertNotIn("info.b_is_frame_package = 1;", module)


if __name__ == "__main__":
    unittest.main()
