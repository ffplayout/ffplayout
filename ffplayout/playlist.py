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
import os
import socket
import time
from copy import deepcopy
from datetime import timedelta
from threading import Thread

import requests

from .filters.default import build_filtergraph
from .utils import (MediaProbe, _general, _playlist, check_sync, get_date,
                    get_float, get_time, messenger, src_or_dummy, stdin_args,
                    str_to_sec, valid_json)


def get_delta(begin):
    """
    get difference between current time and begin from clip in playlist
    """
    current_time = get_time('full_sec')

    if stdin_args.length and str_to_sec(stdin_args.length):
        target_playtime = str_to_sec(stdin_args.length)
    elif _playlist.length:
        target_playtime = _playlist.length
    else:
        target_playtime = 86400.0

    if begin == _playlist.start == 0:
        current_time -= target_playtime

    elif _playlist.start >= current_time and not begin == _playlist.start:
        current_time += target_playtime

    current_delta = begin - current_time

    if math.isclose(current_delta, 86400.0, abs_tol=6):
        current_delta -= 86400.0

    ref_time = target_playtime + _playlist.start
    total_delta = ref_time - begin + current_delta

    return current_delta, total_delta


def handle_list_init(current_delta, total_delta, node):
    """
    # handle init clip, but this clip can be the last one in playlist,
    # this we have to figure out and calculate the right length
    """
    new_seek = abs(current_delta) + node['seek']
    new_out = node['out']

    messenger.debug('List init')

    # don't seek when less the a second
    if 1 > new_seek:
        new_seek = 0

    # when last clip with seek in is longer then total play time, set new out
    if node['out'] - new_seek > total_delta:
        new_out = total_delta + new_seek

    # when total play time is bigger the new length, return seek and out,
    # without asking for new playlist
    if total_delta > new_out - new_seek > 1:
        return new_seek, new_out
    elif new_out - new_seek > 1:
        return new_seek, new_out
    else:
        return 0, 0


def handle_list_end(new_length, node):
    """
    when we come to last clip in playlist,
    or when we reached total playtime,
    we end up here
    """
    new_out = node['out']
    messenger.debug('List end')

    if node['seek'] > 0:
        new_out = node['seek'] + new_length
    else:
        new_out = new_length
    # prevent looping
    if new_out > node['duration']:
        new_out = node['duration']
    else:
        messenger.info(f'We are over time, new length is: {new_length:.2f}')

    missing_secs = abs(new_length - (node['duration'] - node['seek']))

    if node['duration'] > new_length > 1 and \
            node['duration'] - node['seek'] >= new_length:
        node['out'] = new_out
        node = src_or_dummy(node)
    elif node['duration'] > new_length > 0.0:
        time.sleep(new_length)
        messenger.info(
            f'Last clip less then 1 second long, skip:\n{node["source"]}')
        node = None

        if missing_secs > 2:
            messenger.error(
                f'Reach playlist end,\n{missing_secs:.2f} seconds needed.')
    else:
        new_out = node['out']
        node = src_or_dummy(node)
        messenger.error(
            f'Playlist is not long enough:\n{missing_secs:.2f} seconds needed.'
            )

    return node


def timed_source(node, first, last):
    """
    prepare input clip
    check begin and length from clip
    return clip only if we are in 24 hours time range
    """
    current_delta, total_delta = get_delta(node['begin'])

    if first:
        node['seek'], node['out'] = handle_list_init(current_delta,
                                                     total_delta, node)

        if node['out'] > 1.0:
            return src_or_dummy(node)
        else:
            messenger.warning(
                f'Clip less then a second, skip:\n{node["source"]}')
            return None

    else:
        if not stdin_args.loop and _playlist.length:
            check_sync(current_delta)
            messenger.debug(f'current_delta: {current_delta:f}')
            messenger.debug(f'total_delta: {total_delta:f}')

        if (total_delta > node['out'] - node['seek'] and not last) \
                or stdin_args.loop or not _playlist.length:
            # when we are in the 24 houre range, get the clip
            return src_or_dummy(node)

        elif total_delta <= 0:
            messenger.info(
                f'Start time is over playtime, skip clip:\n{node["source"]}')
            return None

        elif total_delta < node['out'] - node['seek'] or last:
            return handle_list_end(total_delta, node)

        else:
            return None


def check_length(total_play_time):
    """
    check if playlist is long enough
    """
    if _playlist.length and total_play_time < _playlist.length - 5 \
            and not stdin_args.loop:
        messenger.error(
            f'Playlist ({get_date(True)}) is not long enough!\n'
            f'Total play time is: {timedelta(seconds=total_play_time)}, '
            f'target length is: {timedelta(seconds=_playlist.length)}'
        )


def validate_thread(clip_nodes):
    """
    validate json values in new thread
    and test if source paths exist
    """
    def check_json(json_nodes):
        error = ''
        counter = 0
        probe = MediaProbe()

        # check if all values are valid
        for node in json_nodes['program']:
            source = node.get('source')
            probe.load(source)
            missing = []
            _in = get_float(node.get('in'), 0)
            _out = get_float(node.get('out'), 0)
            duration = get_float(node.get('duration'), 0)

            if probe.is_remote:
                if not probe.video[0]:
                    missing.append(f'Remote file not exist: "{source}"')
            elif source is None or not os.path.isfile(source):
                missing.append(f'File not exist: "{source}"')

            if not node.get('in') == 0 and not _in:
                missing.append(f'No in Value in: "{node}"')

            if not node.get('out') and not _out:
                missing.append(f'No out Value in: "{node}"')

            if not node.get('duration') and not duration:
                missing.append(f'No duration Value in: "{node}"')

            counter += _out - _in

            line = '\n'.join(missing)
            if line:
                error += line + f'\nIn line: {node}\n\n'

        if error:
            messenger.error(
                'Validation error, check JSON playlist, '
                f'values are missing:\n{error}'
            )

        check_length(counter)

    if clip_nodes.get('program') and len(clip_nodes.get('program')) > 0:
        validate = Thread(
            name='check_json', target=check_json, args=(clip_nodes,))
        validate.daemon = True
        validate.start()
    else:
        messenger.error('Validation error: playlist are empty')


class PlaylistReader:
    def __init__(self, list_date, last_mod_time):
        self.list_date = list_date
        self.last_mod_time = last_mod_time
        self.nodes = None
        self.error = False

    def read(self):
        self.nodes = {'program': []}
        self.error = False

        if stdin_args.playlist:
            json_file = stdin_args.playlist
        else:
            year, month, day = self.list_date.split('-')
            json_file = os.path.join(_playlist.path, year, month,
                                     f'{self.list_date}.json')

        if '://' in json_file:
            json_file = json_file.replace('\\', '/')

            try:
                result = requests.get(json_file, timeout=1, verify=False)
                b_time = result.headers['last-modified']
                temp_time = time.strptime(b_time, "%a, %d %b %Y %H:%M:%S %Z")
                mod_time = time.mktime(temp_time)

                if mod_time > self.last_mod_time:
                    if isinstance(result.json(), dict):
                        self.nodes = result.json()
                    self.last_mod_time = mod_time
                    messenger.info('Open: ' + json_file)
                    validate_thread(deepcopy(self.nodes))
            except (requests.exceptions.ConnectionError, socket.timeout):
                messenger.error(f'No valid playlist from url: {json_file}')
                self.error = True

        elif os.path.isfile(json_file):
            # check last modification from playlist
            mod_time = os.path.getmtime(json_file)
            if mod_time > self.last_mod_time:
                with open(json_file, 'r', encoding='utf-8') as f:
                    self.nodes = valid_json(f)

                self.last_mod_time = mod_time
                messenger.info('Open: ' + json_file)
                validate_thread(deepcopy(self.nodes))
        else:
            messenger.error(f'Playlist not exists: {json_file}')
            self.error = True


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
        if self.node is None:
            return

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

    def eof_handling(self, duration):
        """
        handle except playlist end
        """
        self.node = {
            'begin': get_time('full_sec'),
            'in': 0,
            'seek': 0,
            'out': duration,
            'duration': duration + 1,
            'source': None
        }

        self.generate_cmd()
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
                    self.eof_handling(30)

                else:
                    messenger.error('Playlist not long enough!')
                    self.first = False
                    self.last = False
                    self.eof_handling(60)

            if self.node:
                yield self.node
