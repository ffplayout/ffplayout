#!/usr/bin/env python3

"""
Test script, for testing different situations, like:
    - different day_start times
    - different situ where playlist is empty, not long enough or to long
"""

import json
import os
import sys
from datetime import datetime
from threading import Thread
from time import sleep
from unittest.mock import patch

import time_machine

try:
    from zoneinfo import ZoneInfo
except ImportError:
    from backports.zoneinfo import ZoneInfo

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# from ffplayout import playlist

# set time zone
_TZ = ZoneInfo("Europe/Berlin")


def run_at(time_tuple):
    dt = datetime(*time_tuple, tzinfo=_TZ).strftime("%Y-%m-%d  %H:%M:%S")

    @time_machine.travel(dt)
    def run_in_time_machine():
        desktop.output()

    print(f'simulated date and time: {dt}\n')

    run_in_time_machine()


def run_time(seconds):
    """
    validate json values in new thread
    and test if source paths exist
    """
    def timer(seconds):
        print(f'run test for {seconds} seconds...')
        sleep(seconds)
        terminate_processes()
        print('terminated successfully')

    terminator = Thread(name='timer', target=timer, args=(seconds,))
    terminator.daemon = True
    terminator.start()


def print_separater():
    print('\n')
    print(79 * '-')
    print(79 * '-')


def shorten_playlist(file):
    json_object = json.load(file)
    del json_object['program'][-1:]
    return json_object


def extend_playlist(file):
    json_object = json.load(file)
    elems = json_object['program'][:2]
    json_object['program'].extend(elems)
    return json_object


def clear_playlist(file):
    return {}


@patch('ffplayout.playlist.valid_json', shorten_playlist)
def run_with_less_elements(time_tuple):
    run_at(time_tuple)


@patch('ffplayout.playlist.valid_json', extend_playlist)
def run_with_more_elements(time_tuple):
    run_at(time_tuple)


@patch('ffplayout.playlist.valid_json', clear_playlist)
def run_with_no_elements(time_tuple):
    run_at(time_tuple)


if __name__ == '__main__':
    from ffplayout.output import desktop
    from ffplayout.utils import playlist, terminate_processes

    print('\ntest playlists, which are empty')
    playlist.start = 0
    run_time(140)
    run_with_no_elements((2021, 2, 15, 23, 59, 53))

    print_separater()

    print('\ntest playlists, which are to short')
    playlist.start = 0
    run_time(140)
    run_with_less_elements((2021, 2, 15, 23, 58, 3))

    print_separater()

    print('\ntest playlists, which are to long')
    playlist.start = 0
    run_time(140)
    run_with_more_elements((2021, 2, 15, 23, 59, 33))

    print_separater()

    print('\ntest transition from playlists, with day_start at: 05:59:25')
    playlist.start = 21575
    run_time(140)
    run_at((2021, 2, 17, 5, 58, 3))

    print_separater()

    print('\ntest transition from playlists, with day_start at: 20:00:00')
    playlist.start = 72000
    run_time(140)
    run_at((2021, 2, 17, 19, 58, 23))
