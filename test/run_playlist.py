#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import datetime
import os
import sys
from time import sleep
from zoneinfo import ZoneInfo

import time_machine

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


# set time zone
_TZ = ZoneInfo("Europe/Berlin")
# fake date and time
SOURCE_TIME = [2021, 2, 15, 5, 59, 21]


@time_machine.travel(datetime.datetime(*SOURCE_TIME, tzinfo=_TZ))
def main():
    get_source = GetSourceFromPlaylist()

    for node in get_source.next():
        messenger.info(f'Play: {node["source"]}')
        # print(node)
        sleep(node['out'] - node['seek'])


if __name__ == '__main__':
    from ffplayout.list_reader import GetSourceFromPlaylist
    from ffplayout.utils import messenger
    try:
        main()
    except KeyboardInterrupt:
        print('\n', end='')
