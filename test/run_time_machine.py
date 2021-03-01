#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""
Test script, for simulating different date and time.
This is useful for testing the transition from one playlist to another,
specially when the day_start time is in the night.
"""

import datetime
import os
import sys

import time_machine

try:
    from zoneinfo import ZoneInfo
except ImportError:
    from backports.zoneinfo import ZoneInfo

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# set time zone
_TZ = ZoneInfo("Europe/Berlin")
# fake date and time
SOURCE_TIME = [2021, 2, 14, 5, 58, 5]


@time_machine.travel(datetime.datetime(*SOURCE_TIME, tzinfo=_TZ))
def run_in_time_machine():
    desktop.output()


if __name__ == '__main__':
    from ffplayout.output import desktop
    run_in_time_machine()
