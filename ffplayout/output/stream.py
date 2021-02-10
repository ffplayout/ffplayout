import os
from subprocess import PIPE, Popen
from threading import Thread

from ffplayout.folder import GetSourceFromFolder, MediaStore, MediaWatcher
from ffplayout.playlist import GetSourceFromPlaylist
from ffplayout.utils import (_ff, _log, _playlist, _playout, _pre,
                             _text, ffmpeg_stderr_reader, get_date, messenger,
                             pre_audio_codec, stdin_args, terminate_processes)

_WINDOWS = os.name == 'nt'
COPY_BUFSIZE = 1024 * 1024 if _WINDOWS else 65424


def output():
    """
    this output is for streaming to a target address,
    like rtmp, rtp, svt, etc.
    """
    year = get_date(False).split('-')[0]
    overlay = []

    ff_pre_settings = [
        '-pix_fmt', 'yuv420p', '-r', str(_pre.fps),
        '-c:v', 'mpeg2video', '-intra',
        '-b:v', f'{_pre.v_bitrate}k',
        '-minrate', f'{_pre.v_bitrate}k',
        '-maxrate', f'{_pre.v_bitrate}k',
        '-bufsize', f'{_pre.v_bufsize}k'
        ] + pre_audio_codec() + ['-f', 'mpegts', '-']

    if _text.add_text and not _text.over_pre:
        messenger.info(
            f'Using drawtext node, listening on address: {_text.address}')
        overlay = [
            '-vf',
            "null,zmq=b=tcp\\\\://'{}',drawtext=text='':fontfile='{}'".format(
                _text.address.replace(':', '\\:'), _text.fontfile)
        ]

    try:
        enc_cmd = [
            'ffmpeg', '-v', _log.ff_level.lower(), '-hide_banner',
            '-nostats', '-re', '-thread_queue_size', '256', '-i', 'pipe:0'
            ] + overlay + [
                '-metadata', 'service_name=' + _playout.name,
                '-metadata', 'service_provider=' + _playout.provider,
                '-metadata', f'year={year}'
            ] + _playout.ffmpeg_param + _playout.stream_output

        messenger.debug(f'Encoder CMD: "{" ".join(enc_cmd)}"')

        _ff.encoder = Popen(enc_cmd, stdin=PIPE, stderr=PIPE)

        enc_err_thread = Thread(target=ffmpeg_stderr_reader,
                                args=(_ff.encoder.stderr, False))
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
            for src_cmd, node in get_source.next():
                if watcher is not None:
                    watcher.current_clip = node.get('source')

                messenger.info(f'Play: {node.get("source")}')

                dec_cmd = ['ffmpeg', '-v', _log.ff_level.lower(),
                           '-hide_banner', '-nostats'
                           ] + src_cmd + ff_pre_settings

                messenger.debug(f'Decoder CMD: "{" ".join(dec_cmd)}"')

                with Popen(dec_cmd, stdout=PIPE, stderr=PIPE) as _ff.decoder:
                    dec_err_thread = Thread(target=ffmpeg_stderr_reader,
                                            args=(_ff.decoder.stderr, True))
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
