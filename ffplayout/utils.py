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
This module contains default variables and helper functions
"""

import json
import logging
import math
import re
import shlex
import signal
import smtplib
import socket
import sys
import tempfile
import urllib
from argparse import ArgumentParser
from datetime import date, datetime, timedelta
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from email.utils import formatdate
from logging.handlers import TimedRotatingFileHandler
from pathlib import Path
from platform import system
from shutil import which
from subprocess import STDOUT, CalledProcessError, check_output
from types import SimpleNamespace

import yaml

# path to user define configs
CONFIG_PATH = Path(__file__).parent.absolute().joinpath('conf.d')


# ------------------------------------------------------------------------------
# argument parsing
# ------------------------------------------------------------------------------

stdin_parser = ArgumentParser(description='python and ffmpeg based playout')

stdin_parser.add_argument(
    '-c', '--config', help='file path to ffplayout.conf'
)

stdin_parser.add_argument(
    '-f', '--folder', help='play folder content'
)

stdin_parser.add_argument(
    '-l', '--log', help='file path for logfile'
)

stdin_parser.add_argument(
    '-i', '--loop', help='loop playlist infinitely', action='store_true'
)

stdin_parser.add_argument(
    '-o', '--output', help='set output mode: desktop, hls, stream'
)

stdin_parser.add_argument(
    '-p', '--playlist', help='path from playlist'
)

stdin_parser.add_argument(
    '-s', '--start',
    help='start time in "hh:mm:ss", "now" for start with first'
)

stdin_parser.add_argument(
    '-t', '--length',
    help='set length in "hh:mm:ss", "none" for no length check'
)

stdin_parser.add_argument(
    '-pm', '--play_mode', help='playing mode: folder, playlist, custom...'
)

# read dynamical new arguments
for arg_file in CONFIG_PATH.glob('argparse_*'):
    with open(arg_file, 'r') as _file:
        config = yaml.safe_load(_file)

    short = config.pop('short') if config.get('short') else None
    long = config.pop('long') if config.get('long') else None

    stdin_parser.add_argument(
        *filter(None, [short, long]),
        **config
    )

stdin_args = stdin_parser.parse_args()


# ------------------------------------------------------------------------------
# clock
# ------------------------------------------------------------------------------

def get_time(time_format):
    """
    get different time formats:
        - full_sec > current time in seconds
        - stamp > current date time in seconds
        - or current time in HH:MM:SS
    """
    date_time = datetime.today()

    if time_format == 'full_sec':
        return date_time.hour * 3600 + date_time.minute * 60 \
            + date_time.second + date_time.microsecond / 1000000

    if time_format == 'stamp':
        return float(datetime.now().timestamp())

    return date_time.strftime('%H:%M:%S')


# ------------------------------------------------------------------------------
# default variables and values
# ------------------------------------------------------------------------------

sync_op = SimpleNamespace(time_delta=0, realtime=False)
mail = SimpleNamespace()
log = SimpleNamespace()
pre = SimpleNamespace()
ingest = SimpleNamespace()
playlist = SimpleNamespace()
storage = SimpleNamespace()
lower_third = SimpleNamespace()
playout = SimpleNamespace()

ff_proc = SimpleNamespace(decoder=None, encoder=None)


def str_to_sec(time_str):
    """
    convert time is string in seconds as float
    """
    if time_str in ['now', '', None, 'none']:
        return None

    tms = time_str.split(':')
    try:
        return float(tms[0]) * 3600 + float(tms[1]) * 60 + float(tms[2])
    except ValueError:
        print('Wrong time format!')
        sys.exit(1)


def sec_to_time(seconds):
    """
    convert float number to time string in hh:mm:ss
    """
    min, sec = divmod(seconds, 60)
    hours, min = divmod(min, 60)
    return f'{int(hours):d}:{int(min):02d}:{int(sec):02d}'


def get_float(value, default=False):
    """
    test if value is float
    """
    try:
        return float(value)
    except (ValueError, TypeError):
        return default


def read_config():
    """
    read yaml config
    """

    if stdin_args.config:
        cfg_path = stdin_args.config
    elif Path('/etc/ffplayout/ffplayout.yml').is_file():
        cfg_path = '/etc/ffplayout/ffplayout.yml'
    else:
        cfg_path = 'ffplayout.yml'

    with open(cfg_path, 'r') as config_file:
        return yaml.safe_load(config_file)


def load_config():
    """
    this function can reload most settings from configuration file,
    the change does not take effect immediately, but with the after next file,
    some settings cannot be changed - like resolution, aspect, or output
    """

    cfg = read_config()

    sync_op.threshold = int(cfg['general']['stop_threshold'])

    mail.subject = cfg['mail']['subject']
    mail.server = cfg['mail']['smtp_server']
    mail.port = cfg['mail']['smtp_port']
    mail.s_addr = cfg['mail']['sender_addr']
    mail.s_pass = cfg['mail']['sender_pass']
    mail.recip = cfg['mail']['recipient']
    mail.level = cfg['mail']['mail_level']

    pre.add_logo = cfg['processing']['add_logo']
    pre.logo = cfg['processing']['logo']
    pre.logo_scale = cfg['processing']['logo_scale']
    pre.logo_filter = cfg['processing']['logo_filter']
    pre.logo_opacity = cfg['processing']['logo_opacity']
    pre.add_loudnorm = cfg['processing']['add_loudnorm']
    pre.loud_i = cfg['processing']['loud_I']
    pre.loud_tp = cfg['processing']['loud_TP']
    pre.loud_lra = cfg['processing']['loud_LRA']

    storage.path = cfg['storage']['path']
    storage.filler = cfg['storage']['filler_clip']
    storage.extensions = cfg['storage']['extensions']
    storage.shuffle = cfg['storage']['shuffle']

    lower_third.add_text = cfg['text']['add_text']
    lower_third.over_pre = cfg['text']['over_pre']
    lower_third.address = cfg['text']['bind_address'].replace(':', '\\:')
    lower_third.fontfile = cfg['text']['fontfile']
    lower_third.text_from_filename = cfg['text']['text_from_filename']
    lower_third.style = cfg['text']['style']
    lower_third.regex = cfg['text']['regex']

    return cfg


_cfg = load_config()

if stdin_args.playlist:
    playlist.path = stdin_args.playlist
else:
    playlist.path = _cfg['playlist']['path']

if stdin_args.start is not None:
    playlist.start = str_to_sec(stdin_args.start)
else:
    playlist.start = str_to_sec(_cfg['playlist']['day_start'])

if playlist.start is None:
    playlist.start = get_time('full_sec')

if stdin_args.length:
    playlist.length  = str_to_sec(stdin_args.length)
else:
    playlist.length  = str_to_sec(_cfg['playlist']['length'])

if stdin_args.loop:
    playlist.loop = stdin_args.loop
else:
    playlist.loop = _cfg['playlist']['loop']

log.to_file = _cfg['logging']['log_to_file']
log.backup_count = _cfg['logging']['backup_count']
log.path = Path(_cfg['logging']['log_path'])
log.level = _cfg['logging']['log_level']
log.ff_level = _cfg['logging']['ffmpeg_level']


def pre_audio_codec():
    """
    when add_loudnorm is False we use a different audio encoder,
    s302m has higher quality, but is experimental
    and works not well together with the loudnorm filter
    """
    if pre.add_loudnorm:
        return ['-c:a', 'mp2', '-b:a', '384k', '-ar', '48000', '-ac', '2']

    return ['-c:a', 's302m', '-strict', '-2', '-ar', '48000', '-ac', '2']


ingest.enable = _cfg['ingest']['enable']
ingest.stream_input = shlex.split(_cfg['ingest']['stream_input'])

if stdin_args.play_mode:
    pre.mode = stdin_args.play_mode
else:
    pre.mode = _cfg['processing']['mode']

pre.w = _cfg['processing']['width']
pre.h = _cfg['processing']['height']
pre.aspect = _cfg['processing']['aspect']
pre.fps = _cfg['processing']['fps']
pre.v_bitrate = _cfg['processing']['width'] * _cfg['processing']['height'] / 10
pre.v_bufsize = pre.v_bitrate / 2
pre.output_count = _cfg['processing']['output_count']
pre.buffer_size = 1024 * 1024 if system() == 'Windows' else 65424

pre.settings = [
    '-pix_fmt', 'yuv420p', '-r', str(pre.fps),
    '-c:v', 'mpeg2video', '-g', '1',
    '-b:v', f'{pre.v_bitrate}k',
    '-minrate', f'{pre.v_bitrate}k',
    '-maxrate', f'{pre.v_bitrate}k',
    '-bufsize', f'{pre.v_bufsize}k'
    ] + pre_audio_codec() + ['-f', 'mpegts', '-']

if stdin_args.output:
    playout.mode = stdin_args.output
else:
    playout.mode = _cfg['out']['mode']

playout.preview = _cfg['out']['preview']
playout.preview_param = shlex.split(_cfg['out']['preview_param'])
playout.stream_param = shlex.split(_cfg['out']['stream_param'])


# ------------------------------------------------------------------------------
# logging
# ------------------------------------------------------------------------------

class CustomFormatter(logging.Formatter):
    """
    Logging formatter to add colors and count warning / errors
    """

    grey = '\x1b[38;1m'
    darkgrey = '\x1b[30;1m'
    yellow = '\x1b[33;1m'
    red = '\x1b[31;1m'
    magenta = '\x1b[35;1m'
    green = '\x1b[32;1m'
    blue = '\x1b[34;1m'
    cyan = '\x1b[36;1m'
    reset = '\x1b[0m'

    timestamp = darkgrey + '[%(asctime)s]' + reset
    level = '[%(levelname)s]' + reset
    message = grey + '  %(message)s' + reset

    FORMATS = {
        logging.DEBUG: timestamp + blue + level + '  ' + message + reset,
        logging.INFO: timestamp + green + level + '   ' + message + reset,
        logging.WARNING: timestamp + yellow + level + message + reset,
        logging.ERROR: timestamp + red + level + '  ' + message + reset
    }

    def format_message(self, msg):
        """
        match strings with regex and add different color tags to it
        """
        if '"' in msg:
            msg = re.sub('(".*?")', self.cyan + r'\1' + self.reset, msg)
        elif '[decoder]' in msg:
            msg = re.sub(r'(\[decoder\])', self.reset + r'\1', msg)
        elif '[encoder]' in msg:
            msg = re.sub(r'(\[encoder\])', self.reset + r'\1', msg)
        elif '/' in msg or '\\' in msg:
            msg = re.sub(
                r'("?/[\w.:/]+|["\w.:]+\\.*?)', self.magenta + r'\1', msg)
        elif re.search(r'\d', msg):
            msg = re.sub(
                r'(\d+-\d+-\d+|\d+:\d+:[\d.]+|-?[\d.]+)',
                self.yellow + r'\1' + self.reset, msg)

        return msg

    def format(self, record):
        """
        override logging format
        """
        record.msg = self.format_message(record.getMessage())
        log_fmt = self.FORMATS.get(record.levelno)
        formatter = logging.Formatter(log_fmt)
        return formatter.format(record)


# If the log file is specified on the command line then override the default
if stdin_args.log:
    log.path = stdin_args.log

logger = logging.getLogger('playout')
logger.setLevel(log.level)

if log.to_file and log.path != 'none':
    if log.path.is_dir():
        playout_log = log.path.joinpath('ffplayout.log')
    else:
        log_dir = Path(__file__).parent.parent.absolute().joinpath('log')
        log_dir.mkdir(exist_ok=True)
        playout_log = log_dir.joinpath('ffplayout.log')

    p_format = logging.Formatter('[%(asctime)s] [%(levelname)s]  %(message)s')
    handler = TimedRotatingFileHandler(playout_log, when='midnight',
                                       backupCount=log.backup_count)

    handler.setFormatter(p_format)
    logger.addHandler(handler)
else:
    console_handler = logging.StreamHandler()
    console_handler.setFormatter(CustomFormatter())
    logger.addHandler(console_handler)


# ------------------------------------------------------------------------------
# mail sender
# ------------------------------------------------------------------------------

class Mailer:
    """
    mailer class for sending log messages, with level selector
    """

    def __init__(self):
        self.level = mail.level
        self.time = None
        self.timestamp = get_time('stamp')
        self.rate_limit = 600
        self.temp_msg = Path(tempfile.gettempdir()).joinpath('ffplayout.txt')

    def current_time(self):
        """
        set sending time
        """
        self.time = get_time(None)

    def send_mail(self, msg):
        """
        send emails to specified recipients
        """
        if mail.recip:
            # write message to temp file for rate limit
            with open(self.temp_msg, 'w+') as msg_file:
                msg_file.write(msg)

            self.current_time()

            message = MIMEMultipart()
            message['From'] = mail.s_addr
            message['To'] = mail.recip
            message['Subject'] = mail.subject
            message['Date'] = formatdate(localtime=True)
            message.attach(MIMEText(f'{self.time} {msg}', 'plain'))
            text = message.as_string()

            try:
                server = smtplib.SMTP(mail.server, mail.port)
            except socket.error as err:
                logger.error(err)
                server = None

            if server is not None:
                server.starttls()
                try:
                    login = server.login(mail.s_addr, mail.s_pass)
                except smtplib.SMTPAuthenticationError as serr:
                    logger.error(serr)
                    login = None

                if login is not None:
                    server.sendmail(mail.s_addr,
                                    re.split(', |; |,|;', mail.recip), text)
                    server.quit()

    def check_if_new(self, msg):
        """
        send message only when is new or the rate_limit is pass
        """
        if Path(self.temp_msg).is_file():
            mod_time = Path(self.temp_msg).stat().st_mtime

            with open(self.temp_msg, 'r', encoding='utf-8') as msg_file:
                last_msg = msg_file.read()

                if msg != last_msg \
                        or get_time('stamp') - mod_time > self.rate_limit:
                    self.send_mail(msg)
        else:
            self.send_mail(msg)

    def info(self, msg):
        """
        send emails with level INFO, WARNING and ERROR
        """
        if self.level in ['INFO']:
            self.check_if_new(msg)

    def warning(self, msg):
        """
        send emails with level WARNING and ERROR
        """
        if self.level in ['INFO', 'WARNING']:
            self.check_if_new(msg)

    def error(self, msg):
        """
        send emails with level ERROR
        """
        if self.level in ['INFO', 'WARNING', 'ERROR']:
            self.check_if_new(msg)


class Messenger:
    """
    all logging and mail messages end up here,
    from here they go to logger and mailer
    """

    def __init__(self):
        self._mailer = Mailer()

    # pylint: disable=no-self-use
    def debug(self, msg):
        """
        log debugging messages
        """
        logger.debug(msg.replace('\n', ' '))

    def info(self, msg):
        """
        log and mail info messages
        """
        logger.info(msg.replace('\n', ' '))
        self._mailer.info(msg)

    def warning(self, msg):
        """
        log and mail warning messages
        """
        logger.warning(msg.replace('\n', ' '))
        self._mailer.warning(msg)

    def error(self, msg):
        """
        log and mail error messages
        """
        logger.error(msg.replace('\n', ' '))
        self._mailer.error(msg)


messenger = Messenger()


# ------------------------------------------------------------------------------
# check binaries and ffmpeg libs
# ------------------------------------------------------------------------------

def is_in_system(name):
    """
    Check whether name is on PATH and marked as executable
    """
    if which(name) is None:
        messenger.error(f'{name} is not found on system')
        sys.exit(1)


def ffmpeg_libs():
    """
    check which external libs are compiled in ffmpeg,
    for using them later
    """
    is_in_system('ffmpeg')
    is_in_system('ffprobe')

    cmd = ['ffmpeg', '-filters']
    libs = []
    filters = []

    try:
        info = check_output(cmd, stderr=STDOUT).decode('UTF-8')
    except CalledProcessError as err:
        messenger.error('ffmpeg - libs could not be readed!\n'
                        f'Processing is not possible. Error:\n{err}')
        sys.exit(1)

    for line in info.split('\n'):
        if 'configuration:' in line:
            configs = line.split()

            for cfg in configs:
                if '--enable-lib' in cfg:
                    libs.append(cfg.replace('--enable-', ''))
        elif re.match(r'^(?!.*=) [TSC.]+', line):
            filter_list = line.split()
            if len(filter_list) > 3:
                filters.append(filter_list[1])

    return {'libs': libs, 'filters': filters}


FF_LIBS = ffmpeg_libs()


def validate_ffmpeg_libs():
    """
    check if ffmpeg contains some basic libs
    """
    if 'libx264' not in FF_LIBS['libs']:
        logger.error('ffmpeg contains no libx264!')
    if 'libfdk-aac' not in FF_LIBS['libs']:
        logger.warning(
            'ffmpeg contains no libfdk-aac! No high quality aac...')
    if 'tpad' not in FF_LIBS['filters']:
        logger.error('ffmpeg contains no tpad filter!')
    if 'zmq' not in FF_LIBS['filters']:
        lower_third.add_text = False
        logger.warning(
            'ffmpeg contains no zmq filter!  Text messages will not work...')


# ------------------------------------------------------------------------------
# probe media info's
# ------------------------------------------------------------------------------

class MediaProbe:
    """
    get info's about media file, similar to mediainfo
    """

    def __init__(self):
        self.remote_source = ['http', 'https', 'ftp', 'smb', 'sftp']
        self.src = None
        self.format = {}
        self.audio = []
        self.video = []
        self.is_remote = False

    def load(self, file):
        """
        load media file with ffprobe and get info's out of it
        """
        self.src = file
        self.format = {}
        self.audio = []
        self.video = []

        if self.src and self.src.split('://')[0] in self.remote_source:
            url = self.src.split('://')
            self.src = f'{url[0]}://{urllib.parse.quote(url[1])}'
            self.is_remote = True
        else:
            self.is_remote = False

            if not self.src or not Path(self.src).is_file():
                self.audio.append(None)
                self.video.append(None)

                return

        cmd = ['ffprobe', '-v', 'quiet', '-print_format',
               'json', '-show_format', '-show_streams', self.src]

        try:
            info = json.loads(check_output(cmd).decode('UTF-8'))
        except CalledProcessError as err:
            messenger.error(f'MediaProbe error in: "{self.src}"\n{err}')
            self.audio.append(None)
            self.video.append(None)

            return

        self.format = info['format']

        if get_float(self.format.get('duration'), 0) > 0.1:
            self.format['duration'] = float(self.format['duration'])

        for stream in info['streams']:
            if stream['codec_type'] == 'audio':
                self.audio.append(stream)

            if stream['codec_type'] == 'video':
                if stream.get('display_aspect_ratio'):
                    width, height = stream['display_aspect_ratio'].split(':')
                    stream['aspect'] = float(width) / float(height)
                else:
                    stream['aspect'] = float(
                        stream['width']) / float(stream['height'])

                rate, factor = stream['r_frame_rate'].split('/')
                stream['fps'] = float(rate) / float(factor)

                self.video.append(stream)


# ------------------------------------------------------------------------------
# global helper functions
# ------------------------------------------------------------------------------

def handle_sigterm(sig, frame):
    """
    handler for ctrl+c signal
    """
    raise SystemExit


# pylint: disable=unused-argument
def handle_sighub(sig, frame):
    """
    handling SIGHUP signal for reload configuration
    Linux/macOS only
    """
    messenger.info('Reload config file')
    load_config()


signal.signal(signal.SIGTERM, handle_sigterm)

if system() == 'Linux':
    signal.signal(signal.SIGHUP, handle_sighub)


def terminate_processes(custom_process=None):
    """
    kill orphaned processes
    """
    if ff_proc.decoder and ff_proc.decoder.poll() is None:
        ff_proc.decoder.terminate()

    if ff_proc.encoder and ff_proc.encoder.poll() is None:
        ff_proc.encoder.kill()

    if custom_process:
        custom_process()


def ffmpeg_stderr_reader(std_errors, prefix):
    """
    read ffmpeg stderr decoder and encoder instance
    and log the output
    """
    def form_line(line, level):
        return f'{prefix} {line.replace(level, "").rstrip()}'

    def write_log(line):
        if '[info]' in line:
            logger.info(form_line(line, '[info] '))
        elif '[warning]' in line:
            logger.warning(form_line(line, '[warning] '))
        elif '[error]' in line:
            logger.error(form_line(line, '[error] '))

    try:
        for line in std_errors:
            if log.ff_level == 'INFO':
                write_log(line.decode())
            elif log.ff_level == 'WARNING':
                write_log(line.decode())
            else:
                write_log(line.decode())
    except (ValueError, AttributeError):
        pass


def get_delta(begin):
    """
    get difference between current time and begin from clip in playlist
    """
    current_time = get_time('full_sec')

    if stdin_args.length and str_to_sec(stdin_args.length):
        target_playtime = str_to_sec(stdin_args.length)
    elif playlist.length:
        target_playtime = playlist.length
    else:
        target_playtime = 86400.0

    if begin == playlist.start == 0 and 86400.0 - current_time < 4:
        current_time -= target_playtime

    elif playlist.start >= current_time and not begin == playlist.start:
        current_time += target_playtime

    current_delta = begin - current_time

    if math.isclose(current_delta, 86400.0, abs_tol=sync_op.threshold):
        current_delta -= 86400.0

    ref_time = target_playtime + playlist.start
    total_delta = ref_time - begin + current_delta

    return current_delta, total_delta


def get_date(seek_day, next_start=0):
    """
    get date for correct playlist,
    when seek_day is set:
    check if playlist date must be from yesterday
    """
    date_ = date.today()

    if seek_day and playlist.start > get_time('full_sec'):
        return (date_ - timedelta(1)).strftime('%Y-%m-%d')

    if playlist.start == 0 and next_start >= 86400:
        return (date_ + timedelta(1)).strftime('%Y-%m-%d')

    return date_.strftime('%Y-%m-%d')


def is_advertisement(node):
    """
    check if clip in node is advertisement
    """
    if node and node.get('category') == 'advertisement':
        return True

    return False


def valid_json(file):
    """
    simple json validation
    """
    try:
        json_object = json.load(file)
        return json_object
    except ValueError:
        messenger.error(f'Playlist {file.name} is not JSON conform')
        return None


def check_sync(delta, node=None):
    """
    check that we are in tolerance time
    """

    if pre.mode == 'playlist' and playlist.start and playlist.length:
        # save time delta to global variable for syncing
        # this is needed for real time filter
        sync_op.time_delta = delta

    if abs(delta) > sync_op.threshold > 0:
        messenger.error(
            f'Sync tolerance value exceeded with {delta:.2f} seconds,\n'
            'program terminated!')
        messenger.debug(f'Terminate on node: {node}')
        terminate_processes()
        sys.exit(1)


def check_node_time(node, get_source):
    current_time = get_time('full_sec')
    clip_length = node['out'] - node['seek']
    clip_end = current_time + clip_length

    if pre.mode == 'playlist' and clip_end > current_time:
        get_source.first = True


def seek_in(seek):
    """
    seek in clip
    """
    return ['-ss', str(seek)] if seek > 0.0 else []


def set_length(duration, seek, out):
    """
    set new clip length
    """
    return ['-t', str(out - seek)] if out < duration else []


def loop_input(source, src_duration, target_duration):
    """
    loop files n times
    """
    loop_count = math.ceil(target_duration / src_duration)
    messenger.info(f'Loop "{source}" {loop_count} times, '
                   f'total duration: {target_duration:.2f}')
    return ['-stream_loop', str(loop_count),
            '-i', source, '-t', str(target_duration)]


def gen_dummy(duration):
    """
    generate a dummy clip, with black color and empty audio track
    """
    color = '#121212'
    duration = round(duration, 3)
    # IDEA: add noise could be an config option
    # noise = 'noise=alls=50:allf=t+u,hue=s=0'
    return [
        '-f', 'lavfi', '-i',
        f'color=c={color}:s={pre.w}x{pre.h}:d={duration}:r={pre.fps},'
        'format=pix_fmts=yuv420p',
        '-f', 'lavfi', '-i', f'anoisesrc=d={duration}:c=pink:r=48000:a=0.05'
    ]


def gen_filler(node):
    """
    generate filler clip to fill empty space in playlist
    """
    probe = MediaProbe()
    probe.load(storage.filler)
    duration = node['out'] - node['seek']

    node['probe'] = probe

    if probe.format.get('duration'):
        node['duration'] = probe.format['duration']
        node['source'] = storage.filler
        if node['duration'] > duration:
            # cut filler
            messenger.info(
                f'Generate filler')
            node['src_cmd'] = ['-i', storage.filler] + set_length(
                node['duration'], 0, duration)
            return node

        # loop file n times
        node['src_cmd'] = loop_input(storage.filler, node['duration'],
                                     duration)
        return node

    # when no filler is set, generate a dummy
    messenger.warning('No filler clipt is set! Add dummy...')
    dummy = gen_dummy(duration)
    node['source'] = dummy[3]
    node['src_cmd'] = dummy
    return node


def src_or_dummy(node):
    """
    when source path exist, generate input with seek and out time
    when path not exist, generate dummy clip
    """

    probe = MediaProbe()
    probe.load(node.get('source'))
    node['probe'] = probe

    # check if input is a remote source
    if probe.is_remote and probe.video[0]:
        if node['seek'] > 0.0:
            messenger.warning(
                f'Seek in remote source "{node.get("source")}" not supported!')
        node['src_cmd'] = [
            '-i', node['source']
            ] + set_length(86400, node['seek'], node['out'])
    elif node.get('source') and Path(node['source']).is_file():
        if probe.format.get('duration') and not math.isclose(
                probe.format['duration'], node['duration'], abs_tol=3):
            messenger.debug(
                f"fix duration for: \"{node['source']}\" "
                f"at \"{sec_to_time(node['begin'])}\"")
            node['duration'] = probe.format['duration']

        if node['out'] > node['duration']:
            if node['seek'] > 0.0:
                messenger.warning(
                    f'Seek in looped source "{node["source"]}" not supported!')
                node['src_cmd'] = [
                    '-i', node['source']
                    ] + set_length(node['duration'], node['seek'],
                                   node['out'] - node['seek'])
            else:
                # when list starts with looped clip,
                # the logo length will be wrong
                node['src_cmd'] = loop_input(node['source'], node['duration'],
                                             node['out'])
        else:
            node['src_cmd'] = seek_in(node['seek']) + \
                ['-i', node['source']] + set_length(node['duration'],
                                                    node['seek'], node['out'])
    else:
        if 'source' in node:
            messenger.error(f'File not exist: {node.get("source")}')
        node = gen_filler(node)

    return node

