#!/usr/bin/env python3

"""
Test script, for simulating different date and time.
This is useful for testing the transition from one playlist to another,
specially when the day_start time is in the night.
"""

import datetime
import os
import sys
from importlib import import_module
from unittest.mock import patch
from zoneinfo import ZoneInfo

import time_machine

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# set time zone
_TZ = ZoneInfo("Europe/Berlin")
# fake date and time
SOURCE_TIME = [2022, 1, 5, 5, 57, 10]
FAKE_DELTA = -2.2


def fake_delta(node):
    """
    override list init function for fake delta
    """

    delta, total_delta = get_delta(node['begin'])
    seek = abs(delta) + node['seek'] if abs(delta) + node['seek'] >= 1 else 0
    seek = round(seek, 3)

    seek += FAKE_DELTA

    if node['out'] - seek > total_delta:
        out = total_delta + seek
    else:
        out = node['out']

    if out - seek > 1:
        node['out'] = out
        node['seek'] = seek
        return src_or_dummy(node)

    return None


@patch('ffplayout.player.playlist.handle_list_init', fake_delta)
@time_machine.travel(datetime.datetime(*SOURCE_TIME, tzinfo=_TZ))
def run_in_time_machine():
    if stdin_args.output:
        output = import_module(f'ffplayout.output.{stdin_args.output}').output
        output()
    else:
        desktop.output()


if __name__ == '__main__':
    from ffplayout.output import desktop
    from ffplayout.utils import get_delta, src_or_dummy, stdin_args
    run_in_time_machine()
