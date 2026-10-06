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
    # Firmware defaults may name an 'audio' group that does not exist.
    # Override only that broken named group, never a valid numeric/real group.
    import grp
    import os
    gid_fix = ''
    if alsa.snd_config_search(config, b'defaults.pcm.ipc_gid', c.byref(node)) >= 0:
        alsa.snd_config_get_string.argtypes = [c.c_void_p, c.POINTER(c.c_char_p)]
        value = c.c_char_p()
        if alsa.snd_config_get_string(node, c.byref(value)) == 0 and value.value is not None:
            name = value.value.decode()
            try:
                grp.getgrnam(name)
            except KeyError:
                if not name.isdecimal():
                    gid_fix = f'\ndefaults.pcm.!ipc_gid {os.getgid()}\n'
    # Keep the firmware's default (including Bluetooth), mixer and mic untouched.
    path.write_text(text + '\n# Punktfunk: firmware omitted the speaker Playback alias.\n'
                    'pcm.Playback {\n    type plug\n    slave.pcm "dmix"\n}\n' + gid_fix)
    print('Repaired port-local Spruce Playback alias -> dmix')


if __name__ == '__main__':
    repair(Path(sys.argv[1]))
