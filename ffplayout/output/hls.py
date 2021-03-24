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
import re
from glob import iglob
from subprocess import PIPE, Popen
from threading import Thread

from ffplayout.folder import GetSourceFromFolder, MediaStore, MediaWatcher
from ffplayout.playlist import GetSourceFromPlaylist
from ffplayout.utils import (FF, LOG, PLAYLIST, PLAYOUT, STDIN_ARGS,
                             ffmpeg_stderr_reader, get_date, messenger,
                             terminate_processes)


def clean_ts():
    """
    this function get all *.m3u8 playlists from config,
    read lines from them until it founds first *.ts file,
    then it checks if files on harddrive are older then this first *.ts
    and if so delete them
    """
    playlists = [p for p in PLAYOUT.hls_output if 'm3u8' in p]

    for playlist in playlists:
        messenger.debug(f'cleanup *.ts files from: "{playlist}"')
        test_num = 0
        hls_path = os.path.dirname(playlist)

        if os.path.isfile(playlist):
            with open(playlist, 'r') as m3u8:
                for line in m3u8:
                    if '.ts' in line:
                        test_num = int(re.findall(r'(\d+).ts', line)[0])
                        break

            for ts_file in iglob(os.path.join(hls_path, '*.ts')):
                ts_num = int(re.findall(r'(\d+).ts', ts_file)[0])

                if test_num > ts_num:
                    try:
                        os.remove(ts_file)
                    except OSError:
                        pass


def output():
    """
    this output is hls output, no preprocess is needed.
    """
    year = get_date(False).split('-')[0]

    try:
        if PLAYLIST.mode and not STDIN_ARGS.folder:
            watcher = None
            get_source = GetSourceFromPlaylist()
        else:
            messenger.info('Start folder mode')
            media = MediaStore()
            watcher = MediaWatcher(media)
            get_source = GetSourceFromFolder(media)

        try:
            for node in get_source.next():
                if watcher is not None:
                    watcher.current_clip = node.get('source')

                messenger.info(f'Play: {node.get("source")}')

                cmd = [
                    'ffmpeg', '-v', LOG.ff_level.lower(), '-hide_banner',
                    '-nostats'
                    ] + node['src_cmd'] + node['filter'] + [
                        '-metadata', 'service_name=' + PLAYOUT.name,
                        '-metadata', 'service_provider=' + PLAYOUT.provider,
                        '-metadata', 'year={}'.format(year)
                    ] + PLAYOUT.ffmpeg_param + PLAYOUT.hls_output

                messenger.debug(f'Encoder CMD: "{" ".join(cmd)}"')

                FF.encoder = Popen(cmd, stdin=PIPE, stderr=PIPE)

                stderr_reader_thread = Thread(target=ffmpeg_stderr_reader,
                                              args=(FF.encoder.stderr, False))
                stderr_reader_thread.daemon = True
                stderr_reader_thread.start()
                stderr_reader_thread.join()

                ts_cleaning_thread = Thread(target=clean_ts)
                ts_cleaning_thread.daemon = True
                ts_cleaning_thread.start()

        except BrokenPipeError:
            messenger.error('Broken Pipe!')
            terminate_processes(watcher)

        except SystemExit:
            messenger.info('Got close command')
            terminate_processes(watcher)

        except KeyboardInterrupt:
            messenger.warning('Program terminated')
            terminate_processes(watcher)

        # close encoder when nothing is to do anymore
        if FF.encoder.poll() is None:
            FF.encoder.terminate()

    finally:
        if FF.encoder.poll() is None:
            FF.encoder.terminate()
        FF.encoder.wait()
