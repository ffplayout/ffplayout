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
This module streams to -f null, so it is only for debugging.
"""

from importlib import import_module
from platform import system
from subprocess import PIPE, Popen
from threading import Thread

from ..utils import (ff_proc, ffmpeg_stderr_reader, log, messenger, play,
                     playout, pre, pre_audio_codec, terminate_processes)

COPY_BUFSIZE = 1024 * 1024 if system() == 'Windows' else 65424


def output():
    """
    this output is for streaming to a target address,
    like rtmp, rtp, svt, etc.
    """

    messenger.info(f'Stream to null output, only usefull for debugging...')

    ff_pre_settings = [
        '-pix_fmt', 'yuv420p', '-r', str(pre.fps),
        '-c:v', 'mpeg2video', '-g', '1',
        '-b:v', f'{pre.v_bitrate}k',
        '-minrate', f'{pre.v_bitrate}k',
        '-maxrate', f'{pre.v_bitrate}k',
        '-bufsize', f'{pre.v_bufsize}k'
        ] + pre_audio_codec() + ['-f', 'mpegts', '-']

    try:
        enc_cmd = [
            'ffmpeg', '-v', f'level+{log.ff_level.lower()}', '-hide_banner',
            '-nostats', '-re', '-thread_queue_size', '160', '-i', 'pipe:0'
            ] + playout.stream_param[:-3] + ['-f', 'null', '-']

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
                messenger.info(
                    f'Play for {node["out"] - node["seek"]:.2f} '
                    f'seconds: {node.get("source")}')

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

        except BrokenPipeError:
            messenger.error('Broken Pipe!')
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
