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

"""
This module handles playlists, it can be aware of time syncing.
Empty, missing or any other playlist related failure should be compensate.
Missing clips will be replaced by a dummy clip.
"""

import socket
import time
from copy import deepcopy
from datetime import timedelta
from pathlib import Path
from threading import Thread

import requests

from .filters.default import build_filtergraph
from .utils import (MediaProbe, check_sync, gen_filler, get_date, get_delta,
                    get_float, get_time, messenger, playlist, sec_to_time,
                    src_or_dummy, stdin_args, storage, sync_op, valid_json)


def handle_list_init(node):
    """
    handle init clip, but this clip can be the last one in playlist,
    this we have to figure out and calculate the right length
    """
    messenger.debug('List init')

    delta, total_delta = get_delta(node['begin'])
    seek = abs(delta) + node['seek'] if abs(delta) + node['seek'] >= 1 else 0
    seek = round(seek, 3)

    if node['out'] - seek > total_delta:
        out = total_delta + seek
    else:
        out = node['out']

    if out - seek > 1:
        node['out'] = out
        node['seek'] = seek
        return src_or_dummy(node)

    messenger.warning(f'Clip less then a second, skip:\n{node["source"]}')

    return None


def handle_list_end(duration, node):
    """
    when we come to last clip in playlist,
    or when we reached total playtime,
    we end up here
    """
    messenger.debug('List end')

    out = node['seek'] + duration if node['seek'] > 0 else duration

    # prevent looping
    if out > node['duration']:
        out = node['duration']
    else:
        messenger.warning(
            f'Clip length is not in time, new duration is: {duration:.2f}')

    if node['duration'] > duration > 1 and \
            node['duration'] - node['seek'] >= duration:
        node['out'] = out
        node = src_or_dummy(node)
    elif node['duration'] > duration < 1.0:
        messenger.warning(
            f'Last clip less then 1 second long, skip:\n{node["source"]}')
        node = None
    else:
        _, total_delta = get_delta(node['begin'])
        messenger.error(
            f'Playlist is not long enough:\n{total_delta:.2f} seconds needed')
        node = src_or_dummy(node)

    return node


def timed_source(node, last):
    """
    prepare input clip
    check begin and length from clip
    return clip only if we are in 24 hours time range
    """
    delta, total_delta = get_delta(node['begin'])
    node_ = None

    if not stdin_args.loop and playlist.length:
        messenger.debug(f'delta: {delta:f}')
        messenger.debug(f'total_delta: {total_delta:f}')
        check_sync(delta)

    if (total_delta > node['out'] - node['seek'] and not last) \
            or stdin_args.loop or not playlist.length:
        # when we are in the 24 hour range, get the clip
        node_ = src_or_dummy(node)

    elif total_delta <= 0:
        messenger.info(f'Begin is over play time, skip:\n{node["source"]}')

    elif total_delta < node['duration'] - node['seek'] or last:
        node_ = handle_list_end(total_delta, node)

    return node_


def check_length(total_play_time, list_date):
    """
    check if playlist is long enough
    """
    if playlist.length and total_play_time < playlist.length - 5 \
            and not stdin_args.loop:
        messenger.error(
            f'Playlist from {list_date} is not long enough!\n'
            f'Total play time is: {sec_to_time(total_play_time)}, '
            f'target length is: {sec_to_time(playlist.length)}'
        )


def validate_thread(clip_nodes, list_date):
    """
    validate json values in new thread
    and test if source paths exist
    """
    def check_json(clip_nodes, list_date):
        error = ''
        counter = 0
        probe = MediaProbe()

        # check if all values are valid
        for node in clip_nodes['program']:
            source = node.get('source')
            probe.load(source)
            missing = []
            _in = get_float(node.get('in'), 0)
            _out = get_float(node.get('out'), 0)
            duration = get_float(node.get('duration'), 0)

            if probe.is_remote:
                if not probe.video[0]:
                    missing.append(f'Remote file not exist: "{source}"')
            elif source is None or not Path(source).is_file():
                missing.append(f'File not exist: "{source}", '
                               f'at "{sec_to_time(counter + playlist.start)}"')

            if not type(node.get('in')) in [int, float]:
                missing.append(f'No in Value in: "{node}"')

            if _out == 0:
                missing.append(f'No out Value in: "{node}"')

            if duration == 0:
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

        check_length(counter, list_date)

    if clip_nodes and clip_nodes.get('program') and \
            len(clip_nodes.get('program')) > 0:
        validate = Thread(name='check_json', target=check_json,
                          args=(clip_nodes, list_date))
        validate.daemon = True
        validate.start()
    else:
        messenger.error('Validation error: playlist are empty')


class PlaylistReader:
    """
    Class which read playlists, it checks if playlist got modified,
    when yes it reads the file new, when not it used the cached one
    """

    def __init__(self, list_date, last_mod_time):
        self.list_date = list_date
        self.last_mod_time = last_mod_time
        self.nodes = None
        self.error = False

    def read(self):
        """
        read and process playlist
        """
        self.nodes = {'program': []}
        self.error = False

        if stdin_args.playlist:
            json_file = stdin_args.playlist
        else:
            year, month, _ = self.list_date.split('-')
            json_file = str(Path(playlist.path).joinpath(
                year, month, f'{self.list_date}.json'))

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
                    validate_thread(deepcopy(self.nodes), self.list_date)
            except (requests.exceptions.ConnectionError, socket.timeout):
                messenger.error(f'No valid playlist from url: {json_file}')
                self.error = True

        elif Path(json_file).is_file():
            # check last modification time from playlist
            mod_time = Path(json_file).stat().st_mtime
            if mod_time > self.last_mod_time:
                with open(json_file, 'r', encoding='utf-8') as playlist_file:
                    self.nodes = valid_json(playlist_file)

                self.last_mod_time = mod_time
                messenger.info('Open: ' + json_file)
                validate_thread(deepcopy(self.nodes), self.list_date)
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
        self.prev_date = get_date(True)
        self.list_start = playlist.start
        self.last_time = 0
        self.first = True
        self.last = False
        self.clip_nodes = []
        self.node_count = 0
        self.node = None
        self.prev_node = None
        self.next_node = None
        self.playlist_reader = PlaylistReader(get_date(True), 0.0)
        self.last_error = False

        probe = MediaProbe()
        probe.load(storage.filler)

        self.filler_duration = get_float(probe.format.get('duration'), 60)

    def get_playlist(self):
        """
        read playlist from given date and fill clip_nodes
        when playlist is not available, reset relevant values
        """
        self.playlist_reader.read()

        if self.last_error and not self.playlist_reader.error and \
                self.playlist_reader.list_date == self.prev_date:
            # when last playlist where not exists but now is there and
            # is still the same playlist date,
            # set self.first to true to seek in clip
            # only in this situation seek in is correct!!
            self.first = True
            self.last_error = self.playlist_reader.error

        if self.playlist_reader.nodes and \
                self.playlist_reader.nodes.get('program'):
            self.clip_nodes = self.playlist_reader.nodes.get('program')
            self.node_count = len(self.clip_nodes)

        if self.playlist_reader.error:
            self.clip_nodes = []
            self.node_count = 0
            self.playlist_reader.last_mod_time = 0.0
            self.last_error = self.playlist_reader.error

    def init_time(self):
        """
        get current time in second and shift it when is necessary
        """
        self.last_time = get_time('full_sec')

        if playlist.length:
            total_playtime = playlist.length
        else:
            total_playtime = 86400.0

        if self.last_time < playlist.start:
            self.last_time += total_playtime

    def check_for_next_playlist(self, begin):
        """
        check if playlist length is 24 hours and matches current length,
        to get the date for a new playlist
        """

        if self.node is not None:
            out = self.node['out']
            delta = 0

            if self.node['duration'] > self.node['out']:
                out = self.node['duration']

            if self.last:
                seek = self.node['seek'] if self.node['seek'] > 0 else 0
                delta, _ = get_delta(begin)
                delta += seek + sync_op.threshold

            next_start = begin - playlist.start + out + delta
        else:
            delta, _ = get_delta(begin)
            next_start = begin - playlist.start + sync_op.threshold + delta

        if playlist.length and next_start >= playlist.length:
            self.prev_date = get_date(False, next_start)
            self.playlist_reader.list_date = self.prev_date
            self.playlist_reader.last_mod_time = 0.0
            self.last_time = playlist.start - 1
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
        self.node = timed_source(self.node, self.last)
        if self.node:
            self.node['filter'] = build_filtergraph(self.node, self.prev_node,
                                                    self.next_node)

    def generate_placeholder(self):
        """
        when playlist not exists, or is not long enough,
        generate a placeholder node
        """

        self.init_time()
        begin = self.last_time

        self.node = {
            'begin': begin,
            'number': 0,
            'in': 0,
            'seek': 0,
            'out': self.filler_duration - 0.001,
            'duration': self.filler_duration
        }

        self.generate_cmd()
        self.check_for_next_playlist(begin)

    def eof_handling(self, begin):
        """
        handle except playlist end
        """
        if stdin_args.loop and self.node:
            # when loop parameter is set and playlist node exists,
            # jump to playlist start and play again
            self.list_start = self.last_time + 1
            self.node = None
            messenger.info('Loop playlist')

        elif begin == playlist.start or not self.clip_nodes:
            # playlist not exist or is corrupt/empty
            messenger.error('Clip nodes are empty!')
            self.first = False
            self.generate_placeholder()

        else:
            messenger.error('Playlist not long enough!')
            self.first = False
            self.last = True
            self.generate_placeholder()

    def next(self):
        """
        endless loop for reading playlists
        and getting the right clip node
        """
        while True:
            self.get_playlist()
            begin = self.list_start

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
                    out = self.node['out']

                    if self.node['duration'] > self.node['out']:
                        out = self.node['duration']

                    if self.last_time < begin + out - self.node['seek']:
                        self.previous_and_next_node(index)
                        self.node = handle_list_init(self.node)
                        if self.node:
                            self.node['filter'] = build_filtergraph(
                                self.node, self.prev_node, self.next_node)
                        self.first = False
                        self.last_time = begin

                        self.check_for_next_playlist(begin)
                        break
                elif self.last_time < begin:
                    if index == self.node_count - 1:
                        self.last = True
                    else:
                        self.last = False

                    self.previous_and_next_node(index)
                    self.generate_cmd()
                    self.last_time = begin

                    self.check_for_next_playlist(begin)
                    break

                begin += self.node['out'] - self.node['seek']
            else:
                if not playlist.length and not stdin_args.loop:
                    # when we reach playlist end, stop script
                    messenger.info('Playlist reached end!')
                    return None

                self.eof_handling(begin)

            if self.node:
                yield self.node
