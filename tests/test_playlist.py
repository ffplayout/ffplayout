"""
test classes and functions in playlist.py
"""

import datetime
from zoneinfo import ZoneInfo

import time_machine

from ..ffplayout.player.playlist import (handle_list_end, handle_list_init,
                                            timed_source)
from ..ffplayout.utils import playlist, storage

# set time zone
_TZ = ZoneInfo("Europe/Berlin")


@time_machine.travel(datetime.datetime(*[2021, 5, 31, 6, 0, 20], tzinfo=_TZ))
def test_handle_list_init():
    playlist.start = 6 * 60 * 60
    storage.filler = ''
    node = {'source': '/store/file.mp4', 'begin': 21620,
            'in': 0, 'out': 300, 'duration': 300, 'seek': 20}

    color_src = ('color=c=#121212:s=1024x576:d=280.0:r=25,'
                 'format=pix_fmts=yuv420p')

    list_init = handle_list_init(node)
    list_init.pop('probe')
    check_result = {
        'source': color_src,
        'begin': 21620, 'in': 0, 'out': 300,
        'duration': 300, 'seek': 20.0,
        'src_cmd': [
            '-f', 'lavfi', '-i', color_src,
            '-f', 'lavfi', '-i', 'anoisesrc=d=280.0:c=pink:r=48000:a=0.05']}

    assert list_init == check_result


@time_machine.travel(datetime.datetime(*[2021, 5, 31, 5, 59, 30], tzinfo=_TZ))
def test_handle_list_end():
    playlist.start = 6 * 60 * 60
    storage.filler = ''

    node = {'source': '/store/file.mp4', 'begin': 24 * 3600 - 30,
            'in': 0, 'out': 300, 'duration': 300, 'seek': 0}

    color_src = ('color=c=#121212:s=1024x576:d=30:r=25,'
                 'format=pix_fmts=yuv420p')

    check_result = {
        'source': color_src,
        'begin': 24 * 3600 - 30, 'in': 0, 'out': 30,
        'duration': 300, 'seek': 0,
        'src_cmd': [
            '-f', 'lavfi', '-i', color_src,
            '-f', 'lavfi', '-i', 'anoisesrc=d=30:c=pink:r=48000:a=0.05']}

    list_end = handle_list_end(30, node)
    list_end.pop('probe')

    assert list_end == check_result


@time_machine.travel(datetime.datetime(*[2021, 5, 31, 5, 50, 00], tzinfo=_TZ))
def test_timed_source():
    playlist.start = 6 * 60 * 60
    storage.filler = ''

    node = {'source': '/store/file.mp4', 'begin': 24 * 3600 + 21600 - 600,
            'in': 0, 'out': 300, 'duration': 300, 'seek': 0}

    color_src = ('color=c=#121212:s=1024x576:d=300:r=25,'
                 'format=pix_fmts=yuv420p')

    check_result = {
        'source': color_src,
        'begin': 24 * 3600 + 21600 - 600, 'in': 0, 'out': 300,
        'duration': 300, 'seek': 0,
        'src_cmd': [
            '-f', 'lavfi', '-i', color_src,
            '-f', 'lavfi', '-i', 'anoisesrc=d=300:c=pink:r=48000:a=0.05']}

    src = timed_source(node, False)
    src.pop('probe')

    assert src == check_result
