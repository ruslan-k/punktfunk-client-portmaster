#!/usr/bin/env python3
"""Hand every copied Cedar output on before another network AU is requested.
Run after patch-cedar-pts.py; validate both files before writing either.
"""
from pathlib import Path
import sys
VIDEO_EDITS=[('    /// Demote to software when the presenter cannot display hardware frames.\n',
'''    /// The Cedar rung's escape hatch when the presenter cannot import its dma-buf
    /// planes: drop the hand-off and keep decoding, instead of demoting the whole
    /// rung to software. `false` when there is no such rung or it is already
    /// copying, and the caller demotes.
    pub fn drop_zerocopy(&mut self) -> bool {
        #[cfg(target_os = "linux")]
        if let Backend::NativeCedar(c) = &mut self.backend {
            return c.drop_zerocopy();
        }
        false
    }

    /// Demote to software when the presenter cannot display hardware frames.\n'''),
('    /// Feed one access unit (hosts are one-in/one-out). Hardware errors\n', 
'''    /// Nonblocking Cedar FIFO poll. Other rungs preserve one-output dispatch.
    pub(crate) fn poll_cedar_ready(&mut self) -> Option<DecodedImage> {
        #[cfg(target_os = "linux")]
        if let Backend::NativeCedar(c) = &mut self.backend {
            return c.poll_ready();
        }
        None
    }

    /// Feed one access unit (hosts are one-in/one-out). Hardware errors
''')]
SESSION_EDITS=[
('''                if force_software.swap(false, Ordering::Relaxed) {
                    if let Err(e) = decoder.force_software() {''',
'''                if force_software.swap(false, Ordering::Relaxed) {
                    // A refused dma-buf import is not a dead decoder: the Cedar rung
                    // drops its hand-off and keeps decoding copies, and only a rung
                    // that cannot do that demotes.
                    if decoder.drop_zerocopy() {
                    } else if let Err(e) = decoder.force_software() {'''),
('                    Ok(Some(image)) => {\n',
 '                    Ok(Some(image)) => {\n                        let mut image = image;\n                        loop {\n'),
('                                    kf.ask(Instant::now(), &connector);\n                                    continue;\n',
 '                                    kf.ask(Instant::now(), &connector);\n                                    break;\n'),
('''                        } else {
                            in_flight = Some(next);
                        }
                    }
                    // No output under one-in/one-out LOW_DELAY''',
'''                        } else {
                            in_flight = Some(next);
                        }
                        match decoder.poll_cedar_ready() {
                            Some(next_ready) => image = next_ready,
                            None => break,
                        }
                        }
                    }
                    // No output under one-in/one-out LOW_DELAY'''),
]
def transform(text,edits,name):
    for old,new in edits:
        if text.count(old)!=1:raise ValueError(f'cedar ready: {name} anchor drift; no writes')
        text=text.replace(old,new)
    return text

def main():
    src=Path(sys.argv[1])/'crates/pf-client-core/src'
    writes={}
    for name,edits in [('video.rs',VIDEO_EDITS),('session.rs',SESSION_EDITS)]:
        p=src/name;writes[p]=transform(p.read_text(),edits,name)
    for p,text in writes.items():p.write_text(text)
    print('Cedar ready: nonblocking matched output loop installed')
if __name__=='__main__':main()
