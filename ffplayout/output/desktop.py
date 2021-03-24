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
from subprocess import PIPE, Popen
from threading import Thread

from ffplayout.folder import GetSourceFromFolder, MediaStore, MediaWatcher
from ffplayout.playlist import GetSourceFromPlaylist
from ffplayout.utils import (FF, LOG, PLAYLIST, PRE, STDIN_ARGS, TEXT,
                             ffmpeg_stderr_reader, messenger, pre_audio_codec,
                             terminate_processes)

_WINDOWS = os.name == 'nt'
COPY_BUFSIZE = 1024 * 1024 if _WINDOWS else 65424


def output():
    """
    this output is for playing on desktop with ffplay
    """
    overlay = []

    ff_pre_settings = [
        '-pix_fmt', 'yuv420p', '-r', str(PRE.fps),
        '-c:v', 'mpeg2video', '-intra',
        '-b:v', f'{PRE.v_bitrate}k',
        '-minrate', f'{PRE.v_bitrate}k',
        '-maxrate', f'{PRE.v_bitrate}k',
        '-bufsize', f'{PRE.v_bufsize}k'
        ] + pre_audio_codec() + ['-f', 'mpegts', '-']

    if TEXT.add_text and not TEXT.over_pre:
        messenger.info(
            f'Using drawtext node, listening on address: {TEXT.address}')
        overlay = [
            '-vf',
            "null,zmq=b=tcp\\\\://'{}',drawtext=text='':fontfile='{}'".format(
                TEXT.address.replace(':', '\\:'), TEXT.fontfile)
        ]

    try:
        enc_cmd = [
            'ffplay', '-hide_banner', '-nostats', '-i', 'pipe:0'
            ] + overlay

        messenger.debug(f'Encoder CMD: "{" ".join(enc_cmd)}"')

        FF.encoder = Popen(enc_cmd, stderr=PIPE, stdin=PIPE, stdout=None)

        enc_err_thread = Thread(target=ffmpeg_stderr_reader,
                                args=(FF.encoder.stderr, False))
        enc_err_thread.daemon = True
        enc_err_thread.start()

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

                messenger.info(
                    f'Play for {node["out"] - node["seek"]:.2f} '
                    f'seconds: {node.get("source")}')

                dec_cmd = [
                    'ffmpeg', '-v', LOG.ff_level.lower(),
                    '-hide_banner', '-nostats'
                    ] + node['src_cmd'] + node['filter'] + ff_pre_settings

                messenger.debug(f'Decoder CMD: "{" ".join(dec_cmd)}"')

                with Popen(dec_cmd, stdout=PIPE, stderr=PIPE) as FF.decoder:
                    dec_err_thread = Thread(target=ffmpeg_stderr_reader,
                                            args=(FF.decoder.stderr, True))
                    dec_err_thread.daemon = True
                    dec_err_thread.start()

                    while True:
                        buf = FF.decoder.stdout.read(COPY_BUFSIZE)
                        if not buf:
                            break
                        FF.encoder.stdin.write(buf)

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
