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

import glob
import os
import random
import time
from copy import deepcopy

from watchdog.events import PatternMatchingEventHandler
from watchdog.observers import Observer

from .filters.default import build_filtergraph
from .utils import MediaProbe, _ff, _storage, messenger, stdin_args

# ------------------------------------------------------------------------------
# folder watcher
# ------------------------------------------------------------------------------


class MediaStore:
    """
    fill media list for playing
    MediaWatch will interact with add and remove
    """

    def __init__(self):
        self.store = []

        if stdin_args.folder:
            self.folder = stdin_args.folder
        else:
            self.folder = _storage.path

        self.fill()

    def fill(self):
        for ext in _storage.extensions:
            self.store.extend(
                glob.glob(os.path.join(self.folder, '**', f'*{ext}'),
                          recursive=True))

    def sort_or_radomize(self):
        if _storage.shuffle:
            self.rand()
        else:
            self.sort()

    def add(self, file):
        self.store.append(file)
        self.sort_or_radomize()

    def remove(self, file):
        self.store.remove(file)
        self.sort_or_radomize()

    def sort(self):
        # sort list for sorted playing
        self.store = sorted(self.store)

    def rand(self):
        # randomize list for playing
        random.shuffle(self.store)


class MediaWatcher:
    """
    watch given folder for file changes and update media list
    """

    def __init__(self, media):
        self._media = media
        self.extensions = [f'*{ext}' for ext in _storage.extensions]
        self.current_clip = None

        self.event_handler = PatternMatchingEventHandler(
            patterns=self.extensions)
        self.event_handler.on_created = self.on_created
        self.event_handler.on_moved = self.on_moved
        self.event_handler.on_deleted = self.on_deleted

        self.observer = Observer()
        self.observer.schedule(self.event_handler, self._media.folder,
                               recursive=True)

        self.observer.start()

    def on_created(self, event):
        # add file to media list only if it is completely copied
        file_size = -1
        while file_size != os.path.getsize(event.src_path):
            file_size = os.path.getsize(event.src_path)
            time.sleep(1)

        self._media.add(event.src_path)

        messenger.info(f'Add file to media list: "{event.src_path}"')

    def on_moved(self, event):
        self._media.remove(event.src_path)
        self._media.add(event.dest_path)

        messenger.info(
            f'Move file from "{event.src_path}" to "{event.dest_path}"')

        if self.current_clip == event.src_path:
            _ff.decoder.terminate()

    def on_deleted(self, event):
        self._media.remove(event.src_path)

        messenger.info(f'Remove file from media list: "{event.src_path}"')

        if self.current_clip == event.src_path:
            _ff.decoder.terminate()

    def stop(self):
        self.observer.stop()
        self.observer.join()


class GetSourceFromFolder:
    """
    give next clip, depending on shuffle mode
    """

    def __init__(self, media):
        self._media = media

        self.last_played = []
        self.index = 0
        self.probe = MediaProbe()
        self.next_probe = MediaProbe()
        self.node = None
        self.node_last = None
        self.node_next = None

    def next(self):
        while True:
            while self.index < len(self._media.store):
                if self.node_next:
                    self.node = deepcopy(self.node_next)
                    self.probe = deepcopy(self.next_probe)
                else:
                    self.probe.load(self._media.store[self.index])
                    duration = float(self.probe.format['duration'])
                    self.node = {
                        'in': 0,
                        'out': duration,
                        'duration': duration,
                        'source': self._media.store[self.index]
                    }
                if self.index + 1 < len(self._media.store):
                    self.next_probe.load(self._media.store[self.index + 1])
                    next_duration = float(self.next_probe.format['duration'])
                    self.node_next = {
                        'in': 0,
                        'out': next_duration,
                        'duration': next_duration,
                        'source': self._media.store[self.index + 1]
                    }
                else:
                    self._media.rand()
                    self.next_probe.load(self._media.store[0])
                    next_duration = float(self.next_probe.format['duration'])
                    self.node_next = {
                        'in': 0,
                        'out': next_duration,
                        'duration': next_duration,
                        'source': self._media.store[0]
                    }

                filtergraph = build_filtergraph(
                    self.node, self.node_last, self.node_next, duration,
                    0.0, duration, self.probe)

                yield ['-i', self._media.store[self.index]] + filtergraph, \
                    self.node
                self.index += 1
                self.node_last = deepcopy(self.node)
            else:
                self.index = 0
