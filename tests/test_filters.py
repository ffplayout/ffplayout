"""
test classes and functions in filters/default.py
"""

import argparse
import sys
from types import SimpleNamespace
from unittest.mock import patch

from ..ffplayout.filters.default import (add_audio, add_loudnorm,
                                         custom_filter, deinterlace_filter,
                                         extend_audio, extend_video,
                                         fade_filter, fps_filter,
                                         overlay_filter, pad_filter,
                                         realtime_filter, scale_filter,
                                         split_filter, text_filter)
from ..ffplayout.utils import lower_third, pre, sync_op


def test_text_filter():
    lower_third.add_text = True
    lower_third.over_pre = True
    lower_third.address = '127.0.0.1:5555'
    lower_third.fontfile = ''

    assert text_filter() == [
        "null,zmq=b=tcp\\\\://'127.0.0.1\\:5555',drawtext=text=''"]


def test_deinterlace_filter():
    probe = SimpleNamespace(video=[{'field_order': 'tff'}])

    assert deinterlace_filter(probe) == ['yadif=0:-1:0']


def test_pad_filter():
    probe = SimpleNamespace(video=[{'aspect': 1.333}])

    assert pad_filter(probe) == ['pad=ih*1024/576/sar:ih:(ow-iw)/2:(oh-ih)/2']


def test_fps_filter():
    probe = SimpleNamespace(video=[{'fps': 29.97}])

    assert fps_filter(probe) == ['fps=25']


def test_scale_filter():
    probe = SimpleNamespace(video=[{'width': 1440, 'height': 1080,
                                    'aspect': 1.333}])

    assert scale_filter(probe) == ['scale=1024:576', 'setdar=dar=1.778']


def test_fade_filter():
    assert fade_filter(300, 5, 300) == ['fade=in:st=0:d=0.5']
    assert fade_filter(300, 5, 300, 'a') == ['afade=in:st=0:d=0.5']
    assert fade_filter(300, 0, 200) == ['fade=out:st=199.0:d=1.0']
    assert fade_filter(300, 0, 200, 'a') == ['afade=out:st=199.0:d=1.0']


def test_overlay_filter():
    assert overlay_filter(300, True, False, False) == '[v]null'
    assert overlay_filter(300, False, True, False) == (
        'movie=docs/logo.png,loop=loop=-1:size=1:start=0,format=rgba,'
        'colorchannelmixer=aa=0.7,fade=in:st=0:d=1.0:alpha=1[l];'
        '[v][l]overlay=W-w-12:12:shortest=1')
    assert overlay_filter(300, False, False, True) == (
        'movie=docs/logo.png,loop=loop=-1:size=1:start=0,format=rgba,'
        'colorchannelmixer=aa=0.7,fade=out:st=299:d=1.0:alpha=1[l];'
        '[v][l]overlay=W-w-12:12:shortest=1')
    assert overlay_filter(300, False, False, False) == (
        'movie=docs/logo.png,loop=loop=-1:size=1:start=0,format=rgba,'
        'colorchannelmixer=aa=0.7[l];[v][l]overlay=W-w-12:12:shortest=1')


def test_add_audio():
    probe = SimpleNamespace(audio=False, src='/path/file.mp4')

    assert add_audio(probe, 300) == [
        ('aevalsrc=0:channel_layout=stereo:duration=300:sample_rate=48000')]


def test_add_loudnorm():
    pre.add_loudnorm = True
    pre.loud_i = -18
    pre.loud_tp = -1.5
    pre.loud_lra = 11
    probe = SimpleNamespace(audio=True)

    assert add_loudnorm(probe) == ['loudnorm=I=-18:TP=-1.5:LRA=11']


def test_extend_audio():
    probe = SimpleNamespace(audio=[{'duration': 299}])

    assert extend_audio(probe, 300, 0) == ['apad=whole_dur=300']
    assert extend_audio(probe, 300, 10) == ['apad=whole_dur=290']


def test_extend_video():
    probe = SimpleNamespace(video=[{'duration': 299}])

    assert extend_video(probe, 300, 0) == [
        'tpad=stop_mode=add:stop_duration=1.0']

    assert extend_video(probe, 300, 10) == [
        'tpad=stop_mode=add:stop_duration=1.0']


def test_realtime_filter():
    sync_op.realtime = True

    assert realtime_filter(300) == ',realtime=speed=1'
    assert realtime_filter(300, 'a') == ',arealtime=speed=1'

    sync_op.time_delta = -1.0

    assert realtime_filter(300) == ',realtime=speed=1.0033444816053512'


def test_split_filter():
    pre.output_count = 1
    assert split_filter('v') == '[vout1]'

    pre.output_count = 3
    assert split_filter('v') == ',split=3[vout1][vout2][vout3]'
    assert split_filter('a') == ',asplit=3[aout1][aout2][aout3]'


# pylint: disable=unused-argument
@patch('argparse.ArgumentParser.parse_args', return_value=argparse.Namespace(
    config='', start='', length='', log='', mode='', volume='0.001'))
def test_custom_filter(*args):
    sys.path.append('')
    # lower_third.fontfile = ''
    assert custom_filter('a', None) == ['volume=0.001']
