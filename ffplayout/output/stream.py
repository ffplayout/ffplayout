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
from subprocess import PIPE, Popen
from threading import Thread

from ..ingest_server import ingest_stream
from ..utils import (check_node_time, ff_proc, ffmpeg_stderr_reader, ingest,
                     log, lower_third, messenger, play, playout, pre, sync_op,
                     terminate_processes)


def output():
    """
    this output is for streaming to a target address,
    like rtmp, rtp, svt, etc.
    """
    filtering = []
    node = None
    dec_cmd = []
    preview = []
    live_on = False
    stream_queue = None

    if ingest.enable:
        stream_queue = ingest_stream()

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
            filtering[-1] += ',split=2[v_out1][v_out2]'
            preview = ['-map', '[v_out1]', '-map', '0:a'
                ] + playout.preview_param + ['-map', '[v_out2]', '-map', '0:a']

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
                    ] + node['src_cmd'] + node['filter'] + pre.settings

                messenger.debug(f'Decoder CMD: "{" ".join(dec_cmd)}"')

                with Popen(
                        dec_cmd, stdout=PIPE, stderr=PIPE) as ff_proc.decoder:
                    dec_err_thread = Thread(target=ffmpeg_stderr_reader,
                                            args=(ff_proc.decoder.stderr,
                                                  '[Decoder]'))
                    dec_err_thread.daemon = True
                    dec_err_thread.start()

                    while True:
                        buf_dec = ff_proc.decoder.stdout.read(pre.buffer_size)
                        if stream_queue and not stream_queue.empty():
                            buf_live = stream_queue.get()
                            ff_proc.encoder.stdin.write(buf_live)
                            live_on = True

                            del buf_dec
                        elif buf_dec:
                            ff_proc.encoder.stdin.write(buf_dec)
                        else:
                            if live_on:
                                check_node_time(node, get_source)
                                live_on = False
                            break

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
