import os
import re
from glob import iglob
from subprocess import PIPE, Popen
from threading import Thread

from ffplayout.folder import GetSourceFromFolder, MediaStore, MediaWatcher
from ffplayout.playlist import GetSourceFromPlaylist
from ffplayout.utils import (_current, _ff, _log, _playlist, _playout,
                             ffmpeg_stderr_reader, get_date, messenger,
                             stdin_args, terminate_processes)


def clean_ts():
    """
    this function get all *.m3u8 playlists from config,
    read lines from them until it founds first *.ts file,
    then it checks if files on harddrive are older then this first *.ts
    and if so delete them
    """
    playlists = [p for p in _playout.hls_output if 'm3u8' in p]

    for playlist in playlists:
        messenger.debug('cleanup *.ts files from: "{}"'.format(playlist))
        test_num = 0
        hls_path = os.path.dirname(playlist)
        with open(playlist, 'r') as m3u8:
            for line in m3u8:
                if '.ts' in line:
                    test_num = int(re.findall(r'(\d+).ts', line)[0])
                    break

        for ts_file in iglob(os.path.join(hls_path, '*.ts')):
            ts_num = int(re.findall(r'(\d+).ts', ts_file)[0])

            if test_num > ts_num:
                os.remove(ts_file)


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

                _current.clip = current_file
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

                stderr_reader_thread = Thread(target=ffmpeg_stderr_reader,
                                              args=(_ff.encoder.stderr, False))
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
        if _ff.encoder.poll() is None:
            _ff.encoder.terminate()

    finally:
        if _ff.encoder.poll() is None:
            _ff.encoder.terminate()
        _ff.encoder.wait()
