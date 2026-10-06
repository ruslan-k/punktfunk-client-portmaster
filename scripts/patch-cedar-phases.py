#!/usr/bin/env python3
"""Install opt-in Cedar phase helper and sampled session queue timers atomically."""
from pathlib import Path
import shutil
import sys

START = '    let mut in_flight: Option<InFlight> = None;\n'
START_NEW = START + '''    let cedar_profile = std::env::var("PUNKTFUNK_CEDAR_PROFILE").as_deref() == Ok("1");
    let mut cedar_profile_au = 0u64;
'''
DECODE = '                match decoder.decode_frame(&frame.data, frame.flags, frame.complete) {\n'
DECODE_NEW = '''                let phase_start_ns = cedar_profile.then(now_ns);
                let decoded_result = decoder.decode_frame(&frame.data, frame.flags, frame.complete);
                if let Some(begin) = phase_start_ns {
                    cedar_profile_au += 1;
                    if cedar_profile_au <= 8 || cedar_profile_au.is_multiple_of(120) {
                        tracing::info!(target: "cedar", au = cedar_profile_au,
                            queue_us = begin.saturating_sub(received_ns) / 1000,
                            call_us = now_ns().saturating_sub(begin) / 1000,
                            has_picture = matches!(&decoded_result, Ok(Some(_))),
                            "cedar-session-phase (current input AU; output PTS not correlated)");
                    }
                }
                match decoded_result {
'''


def main():
    root, port = map(Path, sys.argv[1:])
    src = root / 'crates/pf-client-core/src'
    path = src / 'session.rs'
    text = path.read_text()
    for anchor in [START, DECODE]:
        if text.count(anchor) != 1:
            raise SystemExit('cedar phases: session anchor drift; no writes')
    helper = port / 'patches/cedar_phases.rs'
    if not helper.is_file():
        raise SystemExit('cedar phases: missing helper; no writes')
    pts_helper = port / 'patches/cedar_pts.rs'
    if not pts_helper.is_file():
        raise SystemExit('cedar phases: missing PTS helper; no writes')
    tuning_helper = port / 'patches/cedar_tuning.rs'
    if not tuning_helper.is_file():
        raise SystemExit('cedar phases: missing candidate helper; no writes')
    async_helper = port / 'patches/cedar_async.rs'
    if not async_helper.is_file():
        raise SystemExit('cedar phases: missing async helper; no writes')
    text = text.replace(START, START_NEW).replace(DECODE, DECODE_NEW)
    shutil.copyfile(tuning_helper, src / 'cedar_tuning.rs')
    shutil.copyfile(async_helper, src / 'cedar_async.rs')
    shutil.copyfile(pts_helper, src / 'cedar_pts.rs')
    shutil.copyfile(helper, src / 'cedar_phases.rs')
    path.write_text(text)
    print('cedar phases: helper and session timers installed')

if __name__ == '__main__':
    main()
