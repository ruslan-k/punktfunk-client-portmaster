#!/usr/bin/env python3
"""Repair only a missing Spruce speaker alias in the port-local ALSA config."""
import ctypes as c
from pathlib import Path
import sys


def repair(path):
    text = path.read_text()
    if 'playback.pcm "Playback"' not in text:
        return
    alsa = c.CDLL('libasound.so.2')
    alsa.snd_config_update.restype = c.c_int
    alsa.snd_config_search.argtypes = [c.c_void_p, c.c_char_p, c.POINTER(c.c_void_p)]
    alsa.snd_config_search.restype = c.c_int
    if alsa.snd_config_update() < 0:
        raise RuntimeError('ALSA configuration did not load; refusing to modify it')
    config = c.c_void_p.in_dll(alsa, 'snd_config')
    node = c.c_void_p()
    if alsa.snd_config_search(config, b'pcm.Playback', c.byref(node)) >= 0:
        return
    # Keep the firmware's default (including Bluetooth), mixer and mic untouched.
    path.write_text(text + '\n# Punktfunk: firmware omitted the speaker Playback alias.\n'
                    'pcm.Playback {\n    type plug\n    slave.pcm "dmix"\n}\n')
    print('Repaired port-local Spruce Playback alias -> dmix')


if __name__ == '__main__':
    repair(Path(sys.argv[1]))
