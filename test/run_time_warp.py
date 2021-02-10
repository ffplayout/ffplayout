#!/usr/bin/env python3
# -*- coding: utf-8 -*-

# import datetime
import os
import sys
import time
from zoneinfo import ZoneInfo

import time_machine

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# set time zone
_TZ = ZoneInfo("Europe/Berlin")
# fake date and time
# SOURCE_TIME = [2021, 2, 9, 5, 50, 0]

# warp time by factor
WARP_FACTOR = 1000


# @time_machine.travel(datetime.datetime(*SOURCE_TIME, tzinfo=_TZ))
def playlist_test():
    get_source = GetSourceFromPlaylist()
    stamp = time.time()
    duration = 0

    with time_machine.travel(stamp, tick=False) as traveller:
        for src_cmd, node in get_source.next():
            duration = node['out'] - node['seek']
            messenger.info(f'Play: "{node["source"]}"')

            warp_duration = duration / WARP_FACTOR
            messenger.debug(f'warp duration: {warp_duration:.3f}')

            time.sleep(warp_duration)
            stamp += duration

            traveller.move_to(stamp)


if __name__ == '__main__':
    from ffplayout.playlist import GetSourceFromPlaylist
    from ffplayout.utils import messenger

    try:
        playlist_test()
    except KeyboardInterrupt:
        print('Interrupted')
