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

from datetime import datetime, timedelta

from .filters.default import build_filtergraph
from .utils import (MediaProbe, _playlist, gen_filler, get_date, get_delta,
                    get_float, get_time, messenger, read_playlist, stdin_args,
                    timed_source)


class GetSourceFromPlaylist:
    """
    read values from json playlist,
    get current clip in time,
    set ffmpeg source command
    """

    def __init__(self):
        self.list_date = get_date(True)
        self.init_time = _playlist.start
        self.last_time = get_time('full_sec')

        if _playlist.length:
            self.total_playtime = _playlist.length
        else:
            self.total_playtime = 86400.0

        if self.last_time < _playlist.start:
            self.last_time += self.total_playtime

        self.mod_time = 0.0
        self.clip_nodes = None
        self.src_cmd = None
        self.probe = MediaProbe()
        self.filtergraph = []
        self.first = True
        self.last = False
        self.node = None
        self.node_last = None
        self.node_next = None
        self.next_playlist = False
        self.src = None
        self.begin = 0
        self.seek = 0
        self.out = 20
        self.duration = 20

    def get_playlist(self):
        nodes, self.mod_time = read_playlist(self.list_date, self.mod_time)

        self.clip_nodes = nodes if nodes is not None else self.clip_nodes

    def get_input(self):
        self.src_cmd, self.seek, self.out, self.next_playlist = timed_source(
            self.probe, self.src, self.begin, self.duration,
            self.seek, self.out, self.first, self.last
        )

        self.node['seek'] = self.seek
        self.node['out'] = self.out

    def last_and_next_node(self, index):
        if index - 1 >= 0:
            self.node_last = self.clip_nodes['program'][index - 1]
        else:
            self.node_last = None

        if index + 2 <= len(self.clip_nodes['program']):
            self.node_next = self.clip_nodes['program'][index + 1]
        else:
            self.node_next = None

    def set_filtergraph(self):
        self.filtergraph = build_filtergraph(
            self.node, self.node_last, self.node_next,
            self.duration, self.seek, self.out, self.probe)

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
            self.list_date = (
                datetime.strptime(self.list_date, '%Y-%m-%d') + timedelta(1)
                ).strftime('%Y-%m-%d')

            self.mod_time = 0.0
            self.last_time = _playlist.start - 1

    def eof_handling(self, fill, duration=None):
        self.seek = 0.0

        if duration:
            self.out = duration
            self.duration = duration + 1
            self.first = True
        else:
            current_delta, total_delta = get_delta(self.begin)
            self.out = abs(total_delta)
            self.duration = abs(total_delta) + 1
            self.first = False

        self.list_date = get_date(False)
        self.mod_time = 0.0
        self.last_time = 0.0

        if self.out > 2 and fill:
            self.probe, self.src_cmd = gen_filler(self.duration)

            if 'lavfi' in self.src_cmd:
                src = self.src_cmd[3]
            else:
                src = self.src_cmd[1]

            self.node = {
                'in': 0,
                'seek': 0,
                'out': self.out,
                'duration': self.duration,
                'source': src
            }

            self.set_filtergraph()

        else:
            self.src_cmd = None
            self.next_playlist = True

        self.last = False

    def peperation_task(self, index):
        # call functions in order to prepare source and filter
        self.probe.load(self.node.get('source'))
        self.src = self.probe.src

        self.get_input()
        self.last_and_next_node(index)
        self.set_filtergraph()
        self.check_for_next_playlist()

    def next(self):
        while True:
            self.get_playlist()

            if self.clip_nodes is None:
                self.node = {'in': 0, 'out': 30, 'duration': 30}
                messenger.error('clip_nodes are empty')
                self.eof_handling(True, 30)
                yield self.src_cmd + self.filtergraph
                continue

            self.begin = self.init_time

            # loop through all clips in playlist and get correct clip in time
            for index, self.node in enumerate(self.clip_nodes['program']):
                self.seek = get_float(self.node.get('in'), 0)
                self.duration = get_float(self.node.get('duration'), 20)
                self.out = get_float(self.node.get('out'), self.duration)

                # first time we end up here
                if self.first and \
                        self.last_time < self.begin + self.out - self.seek:

                    self.peperation_task(index)
                    self.first = False
                    break
                elif self.last_time < self.begin:
                    if index + 1 == len(self.clip_nodes['program']):
                        self.last = True
                    else:
                        self.last = False

                    self.peperation_task(index)
                    break

                self.begin += self.out - self.seek
            else:
                if stdin_args.loop:
                    self.init_time = self.last_time + 1
                    self.src_cmd = None
                elif not _playlist.length and not stdin_args.loop:
                    # when we reach playlist end, stop script
                    messenger.info('Playlist reached end!')
                    return None
                elif self.begin == self.init_time:
                    # no clip was played, generate dummy
                    messenger.error('Playlist is empty!')
                    self.eof_handling(False)
                else:
                    # playlist is not long enough, play filler
                    messenger.error('Playlist is not long enough!')
                    self.eof_handling(True)

            if self.src_cmd is not None:
                yield self.src_cmd + self.filtergraph, self.node
