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

from watchdog.events import PatternMatchingEventHandler
from watchdog.observers import Observer

from .filters.default import build_filtergraph
from .utils import MediaProbe, _current, _ff, _storage, messenger, stdin_args

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
                glob.glob(os.path.join(self.folder, '**', '*{}'.format(ext)),
                          recursive=True))

        if _storage.shuffle:
            self.rand()
        else:
            self.sort()

    def add(self, file):
        self.store.append(file)
        self.sort()

    def remove(self, file):
        self.store.remove(file)
        self.sort()

    def sort(self):
        # sort list for sorted playing
        self.store = sorted(self.store)

    def rand(self):
        # random sort list for playing
        random.shuffle(self.store)


class MediaWatcher:
    """
    watch given folder for file changes and update media list
    """

    def __init__(self, media):
        self._media = media
        self.extensions = ['*{}'.format(ext) for ext in _storage.extensions]

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

        messenger.info('Add file to media list: "{}"'.format(event.src_path))

    def on_moved(self, event):
        self._media.remove(event.src_path)
        self._media.add(event.dest_path)

        messenger.info('Move file from "{}" to "{}"'.format(event.src_path,
                                                            event.dest_path))

        if _current.clip == event.src_path:
            _ff.decoder.terminate()

    def on_deleted(self, event):
        self._media.remove(event.src_path)

        messenger.info(
            'Remove file from media list: "{}"'.format(event.src_path))

        if _current.clip == event.src_path:
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

    def next(self):
        while True:
            while self.index < len(self._media.store):
                self.probe.load(self._media.store[self.index])
                filtergraph = build_filtergraph(
                    float(self.probe.format['duration']), 0.0,
                    float(self.probe.format['duration']), False, False,
                    False, self.probe, messenger)

                yield [
                    '-i', self._media.store[self.index]
                    ] + filtergraph
                self.index += 1
            else:
                self.index = 0
