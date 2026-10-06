import pathlib
import unittest
ROOT = pathlib.Path(__file__).resolve().parents[1]
class LowDelayTests(unittest.TestCase):
    def test_actual_c_layout_keeps_vendor_copy_and_flag_offset(self):
        import ctypes as c
        import re
        source=(ROOT/'patches/video_cedar.rs').read_text()
        match=re.search(r'struct VideoConfig \{(.*?)\n\}',source,re.S)
        assert match is not None
        block=match.group(1)
        fields=[]
        for name,ty in re.findall(r'^\s*(\w+): ([^,]+),',block,re.M):
            if ty.startswith('*'): value=c.c_void_p
            elif ty.startswith('[u8;'):
                array=re.search(r';\s*(\d+)\]',ty)
                assert array is not None
                value=c.c_uint8*int(array.group(1))
            else: value={'c_int':c.c_int,'c_uint':c.c_uint}[ty]
            fields.append((name,value))
        class Config(c.Structure): _fields_=fields
        self.assertEqual(c.sizeof(Config),216)
        self.assertEqual(Config.common_config_flags_192.offset,192)
        self.assertEqual(Config.n_align_stride.offset,88)

    def test_candidate_is_opt_in_version_and_stream_guarded(self):
        tuning=(ROOT/'patches/cedar_tuning.rs').read_text()
        self.assertIn('PUNKTFUNK_CEDAR_LOW_DELAY', tuning)
        module=(ROOT/'patches/video_cedar.rs').read_text()
        for pin in ['common_config_flags_192: c_uint','low_delay::supports_vendor','low_delay::safe_stream','storage.config.common_config_flags_192 = 1','slice.header.slice_type.is_b()','offset_of!(VideoConfig, common_config_flags_192), 192']:
            self.assertIn(pin,module)
        build=(ROOT/'scripts/build.sh').read_text()
        self.assertIn('cedar_low_delay.rs',build)
        self.assertIn('cedar_low_delay.rs', (ROOT/'scripts/patch-cedar-phases.py').read_text())
