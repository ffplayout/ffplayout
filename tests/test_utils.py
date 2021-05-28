"""
test classes and functions in utils.py
"""


import datetime
from zoneinfo import ZoneInfo

import time_machine

from ffplayout.utils import get_date, playlist

# set time zone
_TZ = ZoneInfo("Europe/Berlin")
# fake date and time
SOURCE_TIME = [2021, 5, 26, 0, 0, 0]


@time_machine.travel(datetime.datetime(*SOURCE_TIME, tzinfo=_TZ))
def playlist_start_zero():
    playlist.start = 0
    assert get_date(False, (24*60*60) + 1) == '2021-05-27'
    assert get_date(False, (24*60*60) - 1) == '2021-05-26'


if __name__ == '__main__':
    playlist_start_zero()
