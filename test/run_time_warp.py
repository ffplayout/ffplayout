#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""
Test script, for simulating speed up the clock.
With the WARP_FACTOR you can transform a second to a fraction.
With this functionality it is possible to run a 24 hours playlist in a minute,
and debug the playlist reader.
"""

import datetime
import os
import sys
import time
from zoneinfo import ZoneInfo

import time_machine

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# set time zone
_TZ = ZoneInfo("Europe/Berlin")
# fake date and time
SOURCE_TIME = [2021, 2, 12, 5, 0, 0]
USE_TIME_MACHINE = True

# warp time by factor
WARP_FACTOR = 1000


def warp_time():
    get_source = GetSourceFromPlaylist()
    stamp = time.time()
    duration = 0
    with time_machine.travel(stamp, tick=False) as traveller:
        for src_cmd, node in get_source.next():
            duration = node['out'] - node['seek']
            messenger.info(f'Play: "{node["source"]}"')

            warp_duration = duration / WARP_FACTOR
            messenger.debug(f'Original duration {duration} '
                            f'warped to {warp_duration:.3f}')

            time.sleep(warp_duration)
            stamp += duration

            traveller.move_to(stamp)


@time_machine.travel(datetime.datetime(*SOURCE_TIME, tzinfo=_TZ))
def run_in_time_machine():
    warp_time()


def run_in_time_warp():
    warp_time()


if __name__ == '__main__':
    from ffplayout.playlist import GetSourceFromPlaylist
    from ffplayout.utils import messenger

    try:
        if USE_TIME_MACHINE:
            run_in_time_machine()
        else:
            warp_time()
    except KeyboardInterrupt:
        print('Interrupted')
