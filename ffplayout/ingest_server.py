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
Start a streaming server and forword it to the playout.
This stream will have the first priority and
play instead of the normal stream (playlist/folder).
"""
from queue import Queue
from subprocess import PIPE, Popen
from threading import Thread
from time import sleep

from .filters.default import overlay_filter
from .utils import ff_proc, ffmpeg_stderr_reader, ingest, messenger, pre


def listener(que):
    filter_ = (f'[0:v]fps={str(pre.fps)},scale={pre.w}:{pre.h},'
               + f'setdar=dar={pre.aspect}[v];')
    filter_ += overlay_filter(0, False, False, False)

    server_cmd = [
        'ffmpeg', '-hide_banner', '-nostats', '-v', 'level+error'
    ] + ingest.input_param + [
        '-filter_complex', f'{filter_}[vout1]',
        '-map', '[vout1]', '-map', '0:a'
    ] + pre.settings

    messenger.warning(
        'Ingest stream is experimental, use it at your own risk!')
    messenger.debug(f'Server CMD: "{" ".join(server_cmd)}"')

    while True:
        with Popen(server_cmd, stderr=PIPE, stdout=PIPE) as ff_proc.server:
            err_thread = Thread(name='stderr_server',
                                target=ffmpeg_stderr_reader,
                                args=(ff_proc.server.stderr, '[Server]'))
            err_thread.daemon = True
            err_thread.start()

            while True:
                buffer = ff_proc.server.stdout.read(pre.buffer_size)
                if not buffer:
                    break

                que.put(buffer)

        sleep(.33)


def ingest_stream():
    streaming_queue = Queue(maxsize=0)

    rtmp_server_thread = Thread(name='ffmpeg_server',target=listener,
                                args=(streaming_queue,))
    rtmp_server_thread.daemon = True
    rtmp_server_thread.start()

    return streaming_queue
