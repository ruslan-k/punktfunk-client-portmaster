#!/usr/bin/env python3
"""Wire the native-cedar decoder rung into the pinned Punktfunk source.

Installs `patches/video_cedar.rs` next to the other pf-client-core sources and
applies six surgical edits (one in `lib.rs`, five in `video.rs`). Each edit's
anchor must occur exactly once in the pinned revision; a mismatch fails before
anything is written, so a half-patched tree cannot reach the compiler.

Usage: patch-cedar.py <punktfunk-checkout> <port-root>
"""
from pathlib import Path
import shutil
import sys

LIB_OLD = '#[cfg(all(desktop, target_os = "linux"))]\npub mod video_vaapi_native;\n'
LIB_NEW = LIB_OLD + (
    "// Native Cedar (Allwinner A523 class): the vendor `libvdecoder` stack is dlopen'd\n"
    "// at session start; H.264 pictures are copied into the CPU planar path. Pin-only:\n"
    "// `PUNKTFUNK_DECODER=native-cedar`.\n"
    "#[cfg(all(desktop, target_os = \"linux\"))]\n"
    "mod video_cedar;\n"
)

ENUM_OLD = "    NativeV4l2(Box<crate::video_v4l2::NativeV4l2Decoder>),\n"
ENUM_NEW = ENUM_OLD + (
    "    /// Allwinner Cedar VPU through the vendor `libvdecoder` stack (A523-class\n"
    "    /// handhelds): H.264 hardware decode copied into the CPU planar path.\n"
    "    /// Pin-only (`native-cedar`); `auto` never reaches it.\n"
    "    #[cfg(target_os = \"linux\")]\n"
    "    NativeCedar(Box<crate::video_cedar::NativeCedarDecoder>),\n"
)

TRIED_OLD = "        let mut native_tried = false;\n"
TRIED_NEW = (
    "        // Native Cedar pin (A523 handhelds). Pin-only: the vendor stack ships on\n"
    "        // the device, not on desktop distros, so `auto` stays upstream's ladder.\n"
    "        // Init failure logs and continues as `auto` — the same contract as every pin.\n"
    "        #[cfg(target_os = \"linux\")]\n"
    "        if choice == crate::video_cedar::DECODER_PIN {\n"
    "            match crate::video_cedar::NativeCedarDecoder::new(wire, stream) {\n"
    "                Ok(d) => {\n"
    "                    tracing::info!(\n"
    "                        codec = codec_name,\n"
    "                        decoder = d.name(),\n"
    "                        \"native Cedar hardware decode active (vendor libvdecoder, copy to CPU planes)\"\n"
    "                    );\n"
    "                    return done(Backend::NativeCedar(Box::new(d)));\n"
    "                }\n"
    "                Err(e) => tracing::warn!(reason = %format!(\"{e:#}\"),\n"
    "                    \"native Cedar init failed — demoting to the standard ladder\"),\n"
    "            }\n"
    "            choice = \"auto\".to_string();\n"
    "        }\n"
) + TRIED_OLD

DECODE_OLD = "            Backend::NativeV4l2(d) => (d.decode(au), d.take_recovery_request()),\n"
DECODE_NEW = DECODE_OLD + (
    "            // Cedar decodes into its own picture buffers and hands over CPU planes\n"
    "            // on the software rung's path; the `stats:` tag is `native-cedar`.\n"
    "            #[cfg(target_os = \"linux\")]\n"
    "            Backend::NativeCedar(c) => (\n"
    "                c.decode(au).map(|f| f.map(DecodedImage::Cpu)),\n"
    "                c.take_recovery_request(),\n"
    "            ),\n"
)

WHICH_OLD = '                    Backend::NativeV4l2(_) => "native V4L2",\n'
WHICH_NEW = WHICH_OLD + (
    "                    #[cfg(target_os = \"linux\")]\n"
    '                    Backend::NativeCedar(_) => "native Cedar",\n'
)

LOGRUNG_OLD = (
    "        Backend::NativeV4l2(_) => (\n"
    "            NativeRung::V4l2.name(),\n"
    "            Some(native_evidence(NativeRung::V4l2, wire)),\n"
    "        ),\n"
)
LOGRUNG_NEW = LOGRUNG_OLD + (
    "        // Native Cedar is pin-only with no evidence-table row: `auto` never\n"
    "        // orders it, so there is no priority to justify here.\n"
    "        #[cfg(target_os = \"linux\")]\n"
    '        Backend::NativeCedar(_) => ("native-cedar", None),\n'
)

EDITS = [
    ("lib.rs module declaration", LIB_OLD, LIB_NEW),
    ("Backend enum arm", ENUM_OLD, ENUM_NEW),
    ("Decoder::new pin block", TRIED_OLD, TRIED_NEW),
    ("decode dispatch arm", DECODE_OLD, DECODE_NEW),
    ("demotion log arm", WHICH_OLD, WHICH_NEW),
    ("log_rung arm", LOGRUNG_OLD, LOGRUNG_NEW),
]


def replace_once(text: str, old: str, new: str, what: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(
            f"cedar patch: anchor for {what} matched {count} times, expected 1 — "
            "the pinned revision drifted; regenerate the anchors"
        )
    return text.replace(old, new)


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: patch-cedar.py <punktfunk-checkout> <port-root>")
    root = Path(sys.argv[1])
    port = Path(sys.argv[2])
    src = root / "crates/pf-client-core/src"
    module_src = port / "patches/video_cedar.rs"
    for path in (src / "lib.rs", src / "video.rs", module_src):
        if not path.is_file():
            raise SystemExit(f"cedar patch: missing {path}")

    lib_text = (src / "lib.rs").read_text(encoding="utf-8")
    video_text = (src / "video.rs").read_text(encoding="utf-8")

    writes: dict[Path, str] = {src / "lib.rs": lib_text, src / "video.rs": video_text}
    for what, old, new in EDITS:
        target = src / "lib.rs" if what.startswith("lib.rs") else src / "video.rs"
        writes[target] = replace_once(writes[target], old, new, what)

    # Everything verified: write the module and the edited files.
    shutil.copyfile(module_src, src / "video_cedar.rs")
    for path, text in writes.items():
        path.write_text(text, encoding="utf-8")
    print("cedar patch: video_cedar.rs installed, 6 upstream edits applied")


if __name__ == "__main__":
    main()
