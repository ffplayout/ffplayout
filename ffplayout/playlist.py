# -*- coding: utf-8 -*-

# This file is part of ffplayout.
#
# ffplayout is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# ffplayout is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with ffplayout. If not, see <http://www.gnu.org/licenses/>.

# ------------------------------------------------------------------------------

import os
import socket
import ssl
import time
from urllib import request

from .filters.default import build_filtergraph
from .utils import (MediaProbe, _playlist, gen_filler, get_date, get_delta,
                    get_time, is_float, messenger, stdin_args, timed_source,
                    valid_json, validate_thread)


class GetSourceFromPlaylist:
    """
    read values from json playlist,
    get current clip in time,
    set ffmpeg source command
    """

    def __init__(self):
        self.init_time = _playlist.start
        self.last_time = get_time('full_sec')

        if _playlist.length:
            self.total_playtime = _playlist.length
        else:
            self.total_playtime = 86400.0

        if self.last_time < _playlist.start:
            self.last_time += self.total_playtime

        self.last_mod_time = 0.0
        self.json_file = None
        self.clip_nodes = None
        self.src_cmd = None
        self.probe = MediaProbe()
        self.filtergraph = []
        self.first = True
        self.last = False
        self.list_date = get_date(True)

        self.src = None
        self.begin = 0
        self.seek = 0
        self.out = 20
        self.duration = 20
        self.ad = False
        self.ad_last = False
        self.ad_next = False

    def get_playlist(self):
        if stdin_args.playlist:
            self.json_file = stdin_args.playlist
        else:
            year, month, day = self.list_date.split('-')
            self.json_file = os.path.join(
             _playlist.path, year, month, self.list_date + '.json')

        if '://' in self.json_file:
            self.json_file = self.json_file.replace('\\', '/')

            try:
                req = request.urlopen(self.json_file,
                                      timeout=1,
                                      context=ssl._create_unverified_context())
                b_time = req.headers['last-modified']
                temp_time = time.strptime(b_time, "%a, %d %b %Y %H:%M:%S %Z")
                mod_time = time.mktime(temp_time)

                if mod_time > self.last_mod_time:
                    self.clip_nodes = valid_json(req)
                    self.last_mod_time = mod_time
                    messenger.info('Open: ' + self.json_file)
                    validate_thread(self.clip_nodes)
            except (request.URLError, socket.timeout):
                self.eof_handling('Get playlist from url failed!', False)

        elif os.path.isfile(self.json_file):
            # check last modification from playlist
            mod_time = os.path.getmtime(self.json_file)
            if mod_time > self.last_mod_time:
                with open(self.json_file, 'r', encoding='utf-8') as f:
                    self.clip_nodes = valid_json(f)

                self.last_mod_time = mod_time
                messenger.info('Open: ' + self.json_file)
                validate_thread(self.clip_nodes)
        else:
            self.clip_nodes = None

    def get_clip_in_out(self, node):
        if is_float(node["in"]):
            self.seek = node["in"]
        else:
            self.seek = 0

        if is_float(node["duration"]):
            self.duration = node["duration"]
        else:
            self.duration = 20

        if is_float(node["out"]):
            self.out = node["out"]
        else:
            self.out = self.duration

    def get_input(self):
        self.src_cmd, self.seek, self.out, self.next_playlist = timed_source(
            self.probe, self.src, self.begin, self.duration,
            self.seek, self.out, self.first, self.last
        )

    def get_category(self, index, node):
        if 'category' in node:
            if index - 1 >= 0:
                last_category = self.clip_nodes[
                    "program"][index - 1]["category"]
            else:
                last_category = 'noad'

            if index + 2 <= len(self.clip_nodes["program"]):
                next_category = self.clip_nodes[
                    "program"][index + 1]["category"]
            else:
                next_category = 'noad'

            if node["category"] == 'advertisement':
                self.ad = True
            else:
                self.ad = False

            if last_category == 'advertisement':
                self.ad_last = True
            else:
                self.ad_last = False

            if next_category == 'advertisement':
                self.ad_next = True
            else:
                self.ad_next = False

    def set_filtergraph(self):
        self.filtergraph = build_filtergraph(
            self.duration, self.seek, self.out, self.ad, self.ad_last,
            self.ad_next, self.probe, messenger)

    def check_for_next_playlist(self):
        if not self.next_playlist:
            # normal behavior, when no new playlist is needed
            self.last_time = self.begin
        elif self.next_playlist and _playlist.length != 86400.0:
            # get sure that no new clip will be loaded
            self.last_time = 86400.0 * 2
        else:
            # when there is no time left and we are in time,
            # set right values for new playlist
            self.list_date = get_date(False)
            self.last_mod_time = 0.0
            self.last_time = _playlist.start - 1

    def eof_handling(self, message, fill, duration=None):
        self.seek = 0.0
        self.ad = False

        messenger.error(message)

        if duration:
            self.out = duration
            self.duration = duration
            self.first = True
        else:
            current_delta, total_delta = get_delta(self.begin)
            self.out = abs(total_delta)
            self.duration = abs(total_delta)
            self.first = False

        self.list_date = get_date(False)
        self.last_mod_time = 0.0
        self.last_time = 0.0

        if self.duration > 2 and fill:
            self.probe, self.src_cmd = gen_filler(self.duration)
            self.set_filtergraph()

        else:
            self.src_cmd = None
            self.next_playlist = True

        self.last = False

    def peperation_task(self, index, node):
        # call functions in order to prepare source and filter
        self.src = node["source"]
        self.probe.load(self.src)

        self.get_input()
        self.get_category(index, node)
        self.set_filtergraph()
        self.check_for_next_playlist()

    def next(self):
        while True:
            self.get_playlist()

            if self.clip_nodes is None:
                self.eof_handling(
                    'No valid playlist:\n{}'.format(self.json_file), True, 30)
                yield self.src_cmd + self.filtergraph
                continue

            self.begin = self.init_time

            # loop through all clips in playlist and get correct clip in time
            for index, node in enumerate(self.clip_nodes["program"]):
                self.get_clip_in_out(node)

                # first time we end up here
                if self.first and \
                        self.last_time < self.begin + self.out - self.seek:

                    self.peperation_task(index, node)
                    self.first = False
                    break
                elif self.last_time < self.begin:
                    if index + 1 == len(self.clip_nodes["program"]):
                        self.last = True
                    else:
                        self.last = False

                    self.peperation_task(index, node)
                    break

                self.begin += self.out - self.seek
            else:
                if stdin_args.loop:
                    self.check_for_next_playlist()
                    self.init_time = self.last_time + 1
                    self.src_cmd = None
                elif not _playlist.length and not stdin_args.loop:
                    # when we reach playlist end, stop script
                    messenger.info('Playlist reached end!')
                    return None
                elif self.begin == self.init_time:
                    # no clip was played, generate dummy
                    self.eof_handling('Playlist is empty!', False)
                else:
                    # playlist is not long enough, play filler
                    self.eof_handling('Playlist is not long enough!', True)

            if self.src_cmd is not None:
                yield self.src_cmd + self.filtergraph
