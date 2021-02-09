#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import datetime
import os
import sys
from zoneinfo import ZoneInfo

import time_machine

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

_tz = ZoneInfo("Europe/Berlin")
source_time = [2021, 2, 8, 23, 59, 50]


@time_machine.travel(datetime.datetime(*source_time, tzinfo=_tz))
def run_in_time_machine():
    try:
        assert datetime.datetime.now() == datetime.datetime(*source_time)
    except AssertionError:
        print('Assertion not possible')
        exit()

    desktop.output()


if __name__ == '__main__':
    from ffplayout.output import desktop
    run_in_time_machine()
