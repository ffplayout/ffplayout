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
This module plays the compressed output directly on the desktop.
"""

from importlib import import_module
from subprocess import PIPE, Popen
from threading import Thread

from ..ingest_server import ingest_stream
from ..utils import (ff_proc, ffmpeg_stderr_reader, ingest,
                     log, lower_third, messenger, pre, terminate_processes)


def output():
    """
    this output is for playing on desktop with ffplay
    """
    overlay = []
    live_on = False
    stream_queue = None

    if ingest.enable:
        stream_queue = ingest_stream()

    if lower_third.add_text and not lower_third.over_pre:
        messenger.info(
            f'Using drawtext node, listening on address: {lower_third.address}'
        )
        overlay = [
            '-vf',
            "null,zmq=b=tcp\\\\://'{}',drawtext=text='':fontfile='{}'".format(
                lower_third.address, lower_third.fontfile)
        ]

    try:
        enc_cmd = [
            'ffplay', '-hide_banner', '-nostats',
            '-v', f'level+{log.ff_level}', '-i', 'pipe:0'
        ] + overlay

        messenger.debug(f'Encoder CMD: "{" ".join(enc_cmd)}"')

        ff_proc.encoder = Popen(enc_cmd, stderr=PIPE, stdin=PIPE, stdout=None)

        enc_err_thread = Thread(target=ffmpeg_stderr_reader,
                                args=(ff_proc.encoder.stderr, '[Encoder]'))
        enc_err_thread.daemon = True
        enc_err_thread.start()

        Iter = import_module(f'ffplayout.player.{pre.mode}').GetSourceIter
        get_source = Iter()

        try:
            for node in get_source.next():
                messenger.info(
                    f'Play for {node["out"] - node["seek"]:.2f} '
                    f'seconds: {node.get("source")}')

                dec_cmd = [
                    'ffmpeg', '-v', f'level+{log.ff_level}',
                    '-hide_banner', '-nostats'
                ] + node['src_cmd'] + node['filter'] + pre.settings

                messenger.debug(f'Decoder CMD: "{" ".join(dec_cmd)}"')

                kill_dec = True

                with Popen(
                        dec_cmd, stdout=PIPE, stderr=PIPE) as ff_proc.decoder:
                    dec_err_thread = Thread(target=ffmpeg_stderr_reader,
                                            args=(ff_proc.decoder.stderr,
                                                  '[Decoder]'))
                    dec_err_thread.daemon = True
                    dec_err_thread.start()

                    while True:
                        if stream_queue and not stream_queue.empty():
                            if kill_dec:
                                kill_dec = False
                                live_on = True
                                get_source.first = True

                                messenger.info(
                                    "Switch from offline source to live ingest")

                                if ff_proc.decoder.poll() is None:
                                    ff_proc.decoder.kill()
                                    ff_proc.decoder.wait()

                            buf_live = stream_queue.get()
                            ff_proc.encoder.stdin.write(buf_live)
                        else:
                            if live_on:
                                messenger.info(
                                    "Switch from live ingest to offline source")
                                kill_dec = True
                                live_on = False

                            buf_dec = ff_proc.decoder.stdout.read(
                                pre.buffer_size)
                            if buf_dec:
                                ff_proc.encoder.stdin.write(buf_dec)
                            else:
                                break

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
            ff_proc.encoder.terminate()

    finally:
        if ff_proc.encoder.poll() is None:
            ff_proc.encoder.terminate()
        ff_proc.encoder.wait()
