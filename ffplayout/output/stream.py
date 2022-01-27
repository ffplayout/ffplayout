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
This module streams the files out to a remote target.
"""

from importlib import import_module
from platform import system
from subprocess import PIPE, Popen
from threading import Thread

from ..utils import (ff_proc, ffmpeg_stderr_reader, log, lower_third,
                     messenger, play, playout, pre, pre_audio_codec, sync_op,
                     terminate_processes)

COPY_BUFSIZE = 1024 * 1024 if system() == 'Windows' else 65424


def output():
    """
    this output is for streaming to a target address,
    like rtmp, rtp, svt, etc.
    """
    filtering = []
    node = None
    dec_cmd = []
    preview = []

    ff_pre_settings = [
        '-pix_fmt', 'yuv420p', '-r', str(pre.fps),
        '-c:v', 'mpeg2video', '-g', '1',
        '-b:v', f'{pre.v_bitrate}k',
        '-minrate', f'{pre.v_bitrate}k',
        '-maxrate', f'{pre.v_bitrate}k',
        '-bufsize', f'{pre.v_bufsize}k'
        ] + pre_audio_codec() + ['-f', 'mpegts', '-']

    if lower_third.add_text and not lower_third.over_pre:
        messenger.info(
            f'Using drawtext node, listening on address: {lower_third.address}'
            )
        filtering = [
            '-filter_complex',
            f"[0:v]null,zmq=b=tcp\\\\://'{lower_third.address}',"
            + f"drawtext=text='':fontfile='{lower_third.fontfile}'"
        ]

        if playout.preview:
            filtering[-1] = ',split=2[v_out1][v_out2]'
            filtering += ['-map', '[v_out2]', '-map', '0:a']
            preview = playout.preview_param

    elif playout.preview:
        preview = playout.preview_param

    try:
        enc_cmd = [
            'ffmpeg', '-v', f'level+{log.ff_level.lower()}', '-hide_banner',
            '-nostats', '-re', '-thread_queue_size', '160', '-i', 'pipe:0'
            ] + filtering + preview + playout.stream_param

        messenger.debug(f'Encoder CMD: "{" ".join(enc_cmd)}"')

        ff_proc.encoder = Popen(enc_cmd, stdin=PIPE, stderr=PIPE)

        enc_err_thread = Thread(target=ffmpeg_stderr_reader,
                                args=(ff_proc.encoder.stderr, '[Encoder]'))
        enc_err_thread.daemon = True
        enc_err_thread.start()

        Iter = import_module(f'ffplayout.player.{play.mode}').GetSourceIter
        get_source = Iter()

        try:
            for node in get_source.next():
                messenger.info(f'Play: {node.get("source")}')

                dec_cmd = [
                    'ffmpeg', '-v', f'level+{log.ff_level.lower()}',
                    '-hide_banner', '-nostats'
                    ] + node['src_cmd'] + node['filter'] + ff_pre_settings

                messenger.debug(f'Decoder CMD: "{" ".join(dec_cmd)}"')

                with Popen(
                        dec_cmd, stdout=PIPE, stderr=PIPE) as ff_proc.decoder:
                    dec_err_thread = Thread(target=ffmpeg_stderr_reader,
                                            args=(ff_proc.decoder.stderr,
                                                  '[Decoder]'))
                    dec_err_thread.daemon = True
                    dec_err_thread.start()

                    while True:
                        buf = ff_proc.decoder.stdout.read(COPY_BUFSIZE)
                        if not buf:
                            break
                        ff_proc.encoder.stdin.write(buf)

        except BrokenPipeError as err:
            messenger.error('Broken Pipe!')
            messenger.debug(79 * '-')
            messenger.debug(f'error: "{err}"')
            messenger.debug(f'delta: "{sync_op.time_delta}"')
            messenger.debug(f'node: "{node}"')
            messenger.debug(f'dec_cmd: "{dec_cmd}"')
            messenger.debug(79 * '-')
            terminate_processes(getattr(get_source, 'stop', None))

        except SystemExit:
            messenger.info('Got close command')
            terminate_processes(getattr(get_source, 'stop', None))

        except KeyboardInterrupt:
            messenger.warning('Program terminated')
            terminate_processes(getattr(get_source, 'stop', None))

        # close encoder when nothing is to do anymore
        if ff_proc.encoder.poll() is None:
            ff_proc.encoder.kill()

    finally:
        if ff_proc.encoder.poll() is None:
            ff_proc.encoder.kill()
        ff_proc.encoder.wait()
