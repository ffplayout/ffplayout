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
from zoneinfo import ZoneInfo

import time_machine

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# set time zone
_TZ = ZoneInfo("Europe/Berlin")
# fake date and time
SOURCE_TIME = [2021, 12, 19, 5, 45, 00]


@time_machine.travel(datetime.datetime(*SOURCE_TIME, tzinfo=_TZ))
def run_in_time_machine():
    if stdin_args.mode:
        output = import_module(f'ffplayout.output.{stdin_args.mode}').output
        output()
    else:
        desktop.output()


if __name__ == '__main__':
    from ffplayout.output import desktop
    from ffplayout.utils import stdin_args
    run_in_time_machine()
