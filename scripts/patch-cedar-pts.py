#!/usr/bin/env python3
"""Route Cedar input/output transport stamps through exact vendor-PTS matches.

Run after patch-cedar-phases.py. Validate every anchor in both files before writes.
"""
from pathlib import Path
import sys

VIDEO_EDITS = [(
    '    /// Feed one access unit (hosts are one-in/one-out). Hardware errors\n',
    '''    #[cfg(target_os = "linux")]
    pub(crate) fn cedar_input_stamp(&mut self, received_ns: u64, pts_ns: u64, flags: u32, frame_index: u32) {
        if let Backend::NativeCedar(c) = &mut self.backend {
            c.set_input_stamp(crate::video_cedar::FrameStamp {
                received_ns, pts_ns, flags, frame_index, ready_ns: 0,
            });
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn take_cedar_output_stamp(&mut self) -> Option<crate::video_cedar::FrameStamp> {
        match &mut self.backend {
            Backend::NativeCedar(c) => c.take_output_stamp(),
            _ => None,
        }
    }

    /// Feed one access unit (hosts are one-in/one-out). Hardware errors
''')]

SESSION_EDITS = [
    ('    received_ns: u64,\n    pts_ns: u64,\n',
     '    received_ns: u64,\n    pts_ns: u64,\n    pixels_ready_ns: Option<u64>,\n'),
    ('    let decoded_ns = now_ns();\n    connector.hud().note_decoded(p.pts_ns, decoded_ns);',
     '    let decoded_ns = p.pixels_ready_ns.unwrap_or_else(now_ns);\n    connector.hud().note_decoded(p.pts_ns, decoded_ns);'),
    ('                let phase_start_ns = cedar_profile.then(now_ns);\n',
     '''                #[cfg(target_os = "linux")]
                decoder.cedar_input_stamp(received_ns, frame.pts_ns, frame.flags, frame.frame_index);
                let phase_start_ns = cedar_profile.then(now_ns);
'''),
    ('                    Ok(Some(image)) => {\n',
     '''                    Ok(Some(image)) => {
                        let (output_received_ns, output_pts_ns, output_flags, pixels_ready_ns) =
                            (received_ns, frame.pts_ns, frame.flags, None);
                        #[cfg(target_os = "linux")]
                        let (output_received_ns, output_pts_ns, output_flags, pixels_ready_ns) =
                            if image.path_label() == "native-cedar" {
                                let Some(stamp) = decoder.take_cedar_output_stamp() else {
                                    tracing::error!(target: "cedar", "Cedar output lacks exact transport stamps; refusing current-AU guess");
                                    kf.ask(Instant::now(), &connector);
                                    continue;
                                };
                                if cedar_profile && (cedar_profile_au <= 16 || cedar_profile_au.is_multiple_of(120)) {
                                    tracing::info!(target: "cedar", input_index = frame.frame_index,
                                        output_index = stamp.frame_index, capture_ns = stamp.pts_ns,
                                        received_ns = stamp.received_ns, ready_ns = stamp.ready_ns,
                                        received_to_ready_us = stamp.ready_ns.saturating_sub(stamp.received_ns) / 1000,
                                        ready_to_handoff_us = now_ns().saturating_sub(stamp.ready_ns) / 1000,
                                        "cedar-correlated-output");
                                }
                                (stamp.received_ns, stamp.pts_ns, stamp.flags, Some(stamp.ready_ns))
                            } else {
                                (output_received_ns, output_pts_ns, output_flags, pixels_ready_ns)
                            };
'''),
    ('                            && frame.flags & punktfunk_core::packet::USER_FLAG_RECOVERY_ANCHOR != 0\n',
     '                            && output_flags & punktfunk_core::packet::USER_FLAG_RECOVERY_ANCHOR != 0\n'),
    ('                        let present = gate.on_decoded_corroborated(\n                            frame.flags,\n',
     '                        let present = gate.on_decoded_corroborated(\n                            output_flags,\n'),
    ('                            received_ns,\n                            pts_ns: frame.pts_ns,\n                            repeat: marks_repeats\n                                && frame.flags & punktfunk_core::packet::USER_FLAG_REPEAT != 0,\n',
     '                            received_ns: output_received_ns,\n                            pts_ns: output_pts_ns,\n                            pixels_ready_ns,\n                            repeat: marks_repeats\n                                && output_flags & punktfunk_core::packet::USER_FLAG_REPEAT != 0,\n'),
    ('cedar-session-phase (current input AU; output PTS not correlated)',
     'cedar-session-phase (input call timer; output correlated separately)'),
]


def transform(text, edits, filename):
    for old, new in edits:
        if text.count(old) != 1:
            raise ValueError(f'cedar PTS: {filename} anchor drift, expected 1 match; no writes')
        text = text.replace(old, new)
    return text


def main():
    root = Path(sys.argv[1])
    src = root/'crates/pf-client-core/src'
    writes = {}
    for name, edits in [('video.rs', VIDEO_EDITS), ('session.rs', SESSION_EDITS)]:
        path = src/name
        writes[path] = transform(path.read_text(), edits, name)
    for path, text in writes.items():
        path.write_text(text)
    print('Cedar PTS: exact metadata, original transport stamps, ready-clock handoff installed')

if __name__ == '__main__':
    main()
