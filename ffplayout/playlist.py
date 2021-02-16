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

import math

from .filters.default import build_filtergraph
from .utils import (PlaylistReader, _general, _playlist, get_date, get_float,
                    get_time, messenger, stdin_args, timed_source)


class GetSourceFromPlaylist:
    """
    read values from json playlist,
    get current clip in time,
    set ffmpeg source command
    """

    def __init__(self):
        self.list_start = _playlist.start
        self.first = True
        self.last = False
        self.clip_nodes = []
        self.node_count = 0
        self.node = None
        self.prev_node = None
        self.next_node = None
        self.playlist = PlaylistReader(get_date(True), 0.0)

    def get_playlist(self):
        """
        read playlist from given date and fill clip_nodes
        when playlist is not available, reset relevant values
        """
        self.playlist.read()

        if self.playlist.nodes.get('program'):
            self.clip_nodes = self.playlist.nodes.get('program')
            self.node_count = len(self.clip_nodes)

        if self.playlist.error:
            self.clip_nodes = []
            self.node_count = 0
            self.playlist.last_mod_time = 0.0

    def init_time(self):
        """
        get current time in second and shift it when is necessary
        """
        self.last_time = get_time('full_sec')

        if _playlist.length:
            total_playtime = _playlist.length
        else:
            total_playtime = 86400.0

        if self.last_time < _playlist.start:
            self.last_time += total_playtime

    def check_for_next_playlist(self):
        """
        check if playlist length is 24 matches current length,
        to get the date for a new playlist
        """
        # when we are in 24 hours range, we can get next playlist

        # calculate the length when current clip is done
        seek = self.node['seek'] if self.first else self.node['in']

        current_length = self.node['begin'] - _playlist.start + (
            self.node['out'] - seek)

        if _playlist.length and self.node and math.isclose(
                _playlist.length, current_length, abs_tol=_general.threshold):

            shift = self.node['out'] - seek
            self.playlist.list_date = get_date(False, shift)
            self.playlist.last_mod_time = 0.0
            self.last_time = _playlist.start - 1
            self.clip_nodes = []

    def previous_and_next_node(self, index):
        """
        set previous and next clip node
        """
        self.prev_node = self.clip_nodes[index - 1] if index > 0 else None

        if index < self.node_count - 1:
            self.next_node = self.clip_nodes[index + 1]
        else:
            self.next_node = None

    def generate_cmd(self):
        """
        extend clip node with ffmpeg source cmd and filters
        """
        self.node = timed_source(self.node, self.first, self.last)
        if self.node:
            self.node['filter'] = build_filtergraph(self.node, self.prev_node,
                                                    self.next_node)

    def eof_handling(self, duration, begin):
        """
        handle except scheduled playlist end
        """
        node_ = {
            'begin': begin,
            'in': 0,
            'seek': 0,
            'out': duration,
            'duration': duration + 1,
            'source': None
        }

        self.node = timed_source(node_, self.first, self.last)
        self.check_for_next_playlist()

    def next(self):
        """
        endless loop for reading playlists
        and getting the right clip node
        """
        while True:
            self.get_playlist()
            begin = _playlist.start

            for index, self.node in enumerate(self.clip_nodes):
                self.node['seek'] = get_float(self.node.get('in'), 0)
                self.node['duration'] = get_float(self.node.get('duration'),
                                                  30)
                self.node['out'] = get_float(self.node.get('out'),
                                             self.node['duration'])
                self.node['begin'] = begin
                self.node['number'] = index + 1

                # first time we end up here
                if self.first:
                    self.init_time()

                    if self.last_time < \
                            begin + self.node['out'] - self.node['seek']:

                        self.previous_and_next_node(index)
                        self.generate_cmd()
                        self.first = False
                        self.last_time = begin

                        self.check_for_next_playlist()
                        break
                elif self.last_time < begin:
                    if index == self.node_count - 1:
                        self.last = True
                    else:
                        self.last = False

                    self.previous_and_next_node(index)
                    self.generate_cmd()
                    self.last_time = begin

                    self.check_for_next_playlist()
                    break

                begin += self.node['out'] - self.node['seek']
            else:
                if stdin_args.loop and self.node:
                    # when loop paramter is set and playlist node exists,
                    # jump to playlist start and play again
                    self.list_start = self.node['begin'] + (
                        self.node['out'] - self.node['seek'])
                    self.node = None
                    messenger.info('Loop playlist')

                elif not _playlist.length and not stdin_args.loop:
                    # when we reach playlist end, stop script
                    # TODO: take next playlist, without sync check
                    messenger.info('Playlist reached end!')
                    return None

                elif begin == _playlist.start or not self.clip_nodes:
                    # playlist not exist or is corrupt/empty
                    messenger.error('Clip nodes are empty!')
                    self.first = True
                    self.last = False
                    self.eof_handling(30, get_time('full_sec'))

                else:
                    messenger.error('Playlist not long enough!')
                    self.first = False
                    self.last = False
                    self.eof_handling(60, begin)

            if self.node:
                yield self.node
