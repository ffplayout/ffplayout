from subprocess import PIPE, Popen
from threading import Thread

from ffplayout.folder import GetSourceFromFolder, MediaStore, MediaWatcher
from ffplayout.playlist import GetSourceFromPlaylist
from ffplayout.utils import (_ff, _log, _playlist, _playout,
                             ffmpeg_stderr_reader, get_date, messenger,
                             stdin_args, terminate_processes)


def output():
    """
    this output is hls output, no preprocess is needed.
    """
    year = get_date(False).split('-')[0]

    try:
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
                cmd = [
                    'ffmpeg', '-v', _log.ff_level.lower(), '-hide_banner',
                    '-nostats'
                    ] + src_cmd + [
                        '-metadata', 'service_name=' + _playout.name,
                        '-metadata', 'service_provider=' + _playout.provider,
                        '-metadata', 'year={}'.format(year)
                    ] + _playout.ffmpeg_param + _playout.hls_output

                _ff.encoder = Popen(cmd, stdin=PIPE, stderr=PIPE)

                enc_thread = Thread(target=ffmpeg_stderr_reader,
                                    args=(_ff.encoder.stderr, True))
                enc_thread.daemon = True
                enc_thread.start()
                enc_thread.join()

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
