import unittest,pathlib,importlib.util,subprocess,tempfile,sys
ROOT=pathlib.Path(__file__).resolve().parents[1]
class CedarReadyTests(unittest.TestCase):
    def test_session_drains_ready_frames_before_waiting_for_next_input(self):
        script=ROOT/'scripts/patch-cedar-ready.py'
        self.assertTrue(script.exists(), 'ready-output handoff patch is missing')
        spec=importlib.util.spec_from_file_location('ready_patch',script)
        assert spec and spec.loader
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        originals={name:'\n'.join(old for old,new in edits) for name,edits in
                   [('video.rs',module.VIDEO_EDITS),('session.rs',module.SESSION_EDITS)]}
        cases=[None]+[(name,old) for name,edits in [('video.rs',module.VIDEO_EDITS),('session.rs',module.SESSION_EDITS)] for old,new in edits]
        for case in cases:
            with tempfile.TemporaryDirectory() as temp:
                src=pathlib.Path(temp)/'crates/pf-client-core/src';src.mkdir(parents=True)
                seed=originals.copy()
                if case:
                    name,old=case;seed[name]=seed[name].replace(old,'ANCHOR DRIFT')
                for name,text in seed.items():(src/name).write_text(text)
                result=subprocess.run([sys.executable,str(script),temp],capture_output=True,text=True)
                if case:
                    self.assertNotEqual(result.returncode,0)
                    self.assertEqual({name:(src/name).read_text() for name in seed},seed)
                else:
                    self.assertEqual(result.returncode,0,result.stderr)
                    text=(src/'session.rs').read_text()
                    self.assertIn('loop {',text)
                    self.assertIn('decoder.poll_cedar_ready()',text)
                    self.assertIn('None => break',text)
                    self.assertIn('kf.ask(Instant::now(), &connector);\n                                    break;',text)
                    self.assertNotIn('kf.ask(Instant::now(), &connector);\n                                    continue;',text)
        build=(ROOT/'scripts/build.sh').read_text()
        self.assertLess(build.index('patch-cedar-pts.py'),build.index('patch-cedar-ready.py'))
        self.assertLess(build.index('patch-cedar-ready.py'),build.index('cargo build --locked'))
        source=(ROOT/'patches/video_cedar.rs').read_text()
        self.assertIn('pub(crate) fn poll_ready',source)
        self.assertIn('output_lag_frames',source)
        self.assertIn('self.output_stamp = output.stamp',source)

    def test_a_refused_import_drops_the_handoff_before_the_rung(self):
        """A refused dma-buf import is not a dead decoder.

        The presenter signals the pump when it cannot display a hardware frame, and
        the pump demoted the whole rung to software - which is why an import failure
        cost 720p60 hardware decode. The Cedar rung can hand copies instead, so the
        demotion asks it to drop zero-copy first and only falls through when there is
        nothing to drop.
        """
        script=ROOT/'scripts/patch-cedar-ready.py'
        spec=importlib.util.spec_from_file_location('ready_patch',script)
        assert spec and spec.loader
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        edits={name:dict(edits) for name,edits in
               [('video.rs',module.VIDEO_EDITS),('session.rs',module.SESSION_EDITS)]}
        session=edits['session.rs']
        demote=next(old for old in session if 'force_software.swap(false' in old)
        applied=session[demote]
        self.assertIn('if decoder.drop_zerocopy()',applied)
        self.assertLess(applied.index('decoder.drop_zerocopy()'),applied.index('force_software()'))
        video=edits['video.rs']
        drop=next(old for old in video if 'cannot display hardware frames' in old)
        applied_video=video[drop]
        self.assertIn('pub fn drop_zerocopy(&mut self) -> bool',applied_video)
        self.assertIn('if let Backend::NativeCedar(c) = &mut self.backend {',applied_video)
        self.assertIn('return c.drop_zerocopy();',applied_video)
        source=(ROOT/'patches/video_cedar.rs').read_text()
        self.assertIn('pub(crate) fn drop_zerocopy(&mut self) -> bool',source)
        self.assertIn('self.zerocopy = false;',source)
        env=(ROOT/'package/punktfunk/runtime-env.sh').read_text()
        self.assertIn('export PUNKTFUNK_CEDAR_ZEROCOPY=${PUNKTFUNK_CEDAR_ZEROCOPY:-1}',env)
        cfg=(ROOT/'package/punktfunk/config.env').read_text()
        self.assertIn('#PUNKTFUNK_CEDAR_ZEROCOPY=1',cfg)
