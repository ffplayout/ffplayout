#!/usr/bin/env python3
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
from subprocess import PIPE, Popen
from threading import Thread

from ffplayout.folder import GetSourceFromFolder, MediaStore, MediaWatcher
from ffplayout.playlist import GetSourceFromPlaylist
from ffplayout.utils import (COPY_BUFSIZE, DEC_PREFIX, ENC_PREFIX, _ff, _log,
                             _playlist, _playout, _pre_comp, _text,
                             decoder_logger, encoder_logger,
                             ffmpeg_stderr_reader, get_date, messenger,
                             pre_audio_codec, stdin_args, terminate_processes)

try:
    if os.name != 'posix':
        import colorama
        colorama.init()
except ImportError:
    print('Some modules are not installed, ffplayout may or may not work')


# ------------------------------------------------------------------------------
# main functions
# ------------------------------------------------------------------------------

def main():
    """
    pipe ffmpeg pre-process to final ffmpeg post-process,
    or play with ffplay
    """
    year = get_date(False).split('-')[0]
    overlay = []

    ff_pre_settings = [
        '-pix_fmt', 'yuv420p', '-r', str(_pre_comp.fps),
        '-c:v', 'mpeg2video', '-intra',
        '-b:v', '{}k'.format(_pre_comp.v_bitrate),
        '-minrate', '{}k'.format(_pre_comp.v_bitrate),
        '-maxrate', '{}k'.format(_pre_comp.v_bitrate),
        '-bufsize', '{}k'.format(_pre_comp.v_bufsize)
        ] + pre_audio_codec() + ['-f', 'mpegts', '-']

    if _text.add_text:
        messenger.info('Using drawtext node, listening on address: {}'.format(
            _text.address
        ))
        overlay = [
            '-vf', "null,zmq=b='{}',drawtext=text='':fontfile='{}'".format(
                _text.address.replace(':', '\\:'), _text.fontfile)
        ]

    try:
        if _playout.preview or stdin_args.desktop:
            # preview playout to player
            _ff.encoder = Popen([
                'ffplay', '-hide_banner', '-nostats', '-i', 'pipe:0'
                ] + overlay, stderr=PIPE, stdin=PIPE, stdout=None)
        else:
            _ff.encoder = Popen([
                'ffmpeg', '-v', _log.ff_level.lower(), '-hide_banner',
                '-nostats', '-re', '-thread_queue_size', '256',
                '-i', 'pipe:0'] + overlay + [
                    '-metadata', 'service_name=' + _playout.name,
                    '-metadata', 'service_provider=' + _playout.provider,
                    '-metadata', 'year={}'.format(year)
                ] + _playout.post_comp_param + [_playout.out_addr],
                stdin=PIPE, stderr=PIPE)

        enc_err_thread = Thread(target=ffmpeg_stderr_reader,
                                args=(_ff.encoder.stderr, encoder_logger,
                                      ENC_PREFIX))
        enc_err_thread.daemon = True
        enc_err_thread.start()

        if _playlist.mode and not stdin_args.folder:
            watcher = None
            get_source = GetSourceFromPlaylist()
        else:
            messenger.info('Start folder mode')
            media = MediaStore()
            watcher = MediaWatcher(media)
            get_source = GetSourceFromFolder(media)

        try:
            for src_cmd in get_source.next():
                messenger.debug('src_cmd: "{}"'.format(src_cmd))
                if src_cmd[0] == '-i':
                    current_file = src_cmd[1]
                else:
                    current_file = src_cmd[3]

                messenger.info('Play: "{}"'.format(current_file))

                with Popen([
                    'ffmpeg', '-v', _log.ff_level.lower(), '-hide_banner',
                    '-nostats'] + src_cmd + ff_pre_settings,
                        stdout=PIPE, stderr=PIPE) as _ff.decoder:

                    dec_err_thread = Thread(target=ffmpeg_stderr_reader,
                                            args=(_ff.decoder.stderr,
                                                  decoder_logger,
                                                  DEC_PREFIX))
                    dec_err_thread.daemon = True
                    dec_err_thread.start()

                    while True:
                        buf = _ff.decoder.stdout.read(COPY_BUFSIZE)
                        if not buf:
                            break
                        _ff.encoder.stdin.write(buf)

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
        if _ff.encoder.poll() is None:
            _ff.encoder.terminate()

    finally:
        if _ff.encoder.poll() is None:
            _ff.encoder.terminate()
        _ff.encoder.wait()


if __name__ == '__main__':
    main()
