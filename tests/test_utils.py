"""
test classes and functions in utils.py
"""


import datetime
from zoneinfo import ZoneInfo

import time_machine

from ..ffplayout.utils import (gen_dummy, gen_filler, get_date, get_delta,
                               get_float, is_advertisement, loop_input,
                               playlist, pre, seek_in, set_length,
                               src_or_dummy, str_to_sec)

# set time zone
_TZ = ZoneInfo("Europe/Berlin")


def test_str_to_sec():
    assert str_to_sec('06:00:00') == 21600


@time_machine.travel(datetime.datetime(*[2021, 5, 26, 15, 30, 5], tzinfo=_TZ))
def test_get_delta():
    playlist.start = 0
    current, total = get_delta(15 * 3600 + 30 * 60 + 1)
    assert current == -4
    assert total == 8 * 3600 + 29 * 60 + 55


@time_machine.travel(datetime.datetime(*[2021, 5, 26, 0, 0, 0], tzinfo=_TZ))
def test_playlist_start_zero():
    playlist.start = 0
    assert get_date(False, 24 * 60 * 60 + 1) == '2021-05-27'
    assert get_date(False, 24 * 60 * 60 - 1) == '2021-05-26'


@time_machine.travel(datetime.datetime(*[2021, 5, 26, 5, 59, 59], tzinfo=_TZ))
def test_playlist_start_six_before():
    playlist.start = 6 * 60 * 60
    assert get_date(True) == '2021-05-25'
    assert get_date(False) == '2021-05-26'


@time_machine.travel(datetime.datetime(*[2021, 5, 26, 6, 0, 0], tzinfo=_TZ))
def test_playlist_start_six_after():
    playlist.start = 6 * 60 * 60
    assert get_date(False) == '2021-05-26'


def test_get_float():
    assert get_float('5') == 5
    assert get_float('5', None) == 5.0
    assert get_float('5a', None) is None


def test_is_advertisement():
    assert is_advertisement({'category': 'advertisement'}) is True
    assert is_advertisement({'category': ''}) is False
    assert is_advertisement({}) is False


def test_seek_in():
    assert seek_in(10) == ['-ss', '10']
    assert seek_in(0) == []


def test_set_length():
    assert set_length(300, 50, 200) == ['-t', '150']
    assert set_length(300, 0, 300) == []


def test_loop_input():
    assert loop_input('/store/file.mp4', 300, 450) == ['-stream_loop', '2',
                                                       '-i', '/store/file.mp4',
                                                       '-t', '450']


def test_gen_dummy():
    pre.w = 1024
    pre.h = 576
    pre.fps = 25
    assert gen_dummy(30) == ['-f', 'lavfi', '-i',
                             'color=c=#121212:s=1024x576:d=30:r=25,'
                             'format=pix_fmts=yuv420p', '-f', 'lavfi',
                             '-i', 'anoisesrc=d=30:c=pink:r=48000:a=0.05']


def test_gen_filler():
    source = gen_filler({'source': '/store/file.mp4',
                         'in': 0, 'out': 300, 'duration': 300, 'seek': 0})
    filler = {'duration': 300, 'in': 0, 'out': 300, 'seek': 0,
              'source': '/tv-media/ADtv/01 - Intro/seperator.clock.5-00.mp4',
              'src_cmd': [
                  '-i', '/tv-media/ADtv/01 - Intro/seperator.clock.5-00.mp4',
                  '-t', '300']}

    source.pop('probe')

    assert source == filler


def test_src_or_dummy():
    source = src_or_dummy({'source': '/store/file.mp4',
                           'in': 0, 'out': 300, 'duration': 300, 'seek': 0})

    dummy = {'duration': 300, 'in': 0, 'out': 300, 'seek': 0,
             'source': '/tv-media/ADtv/01 - Intro/seperator.clock.5-00.mp4',
             'src_cmd': [
                '-i', '/tv-media/ADtv/01 - Intro/seperator.clock.5-00.mp4',
                '-t', '300']}

    source.pop('probe')

    assert source == dummy
