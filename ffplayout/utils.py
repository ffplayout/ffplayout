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

import json
import logging
import math
import os
import re
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
from glob import glob
from logging.handlers import TimedRotatingFileHandler
from shutil import which
from subprocess import STDOUT, CalledProcessError, check_output
from types import SimpleNamespace

import yaml

# path to user define configs
CONFIG_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                           'conf.d')

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
    '-m', '--mode', help='set output mode: desktop, hls, stream'
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

# read dynamical new arguments
for arg_file in glob(os.path.join(CONFIG_PATH, 'argparse_*')):
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
        - else > current time in HH:MM:SS
    """
    t = datetime.today()

    if time_format == 'full_sec':
        return t.hour * 3600 + t.minute * 60 + t.second \
             + t.microsecond / 1000000
    elif time_format == 'stamp':
        return float(datetime.now().timestamp())
    else:
        return t.strftime('%H:%M:%S')


# ------------------------------------------------------------------------------
# default variables and values
# ------------------------------------------------------------------------------

_general = SimpleNamespace()
_mail = SimpleNamespace()
_log = SimpleNamespace()
_pre = SimpleNamespace()
_playlist = SimpleNamespace()
_storage = SimpleNamespace()
_text = SimpleNamespace()
_playout = SimpleNamespace()

_init = SimpleNamespace(load=True)
_ff = SimpleNamespace(decoder=None, encoder=None)
_global = SimpleNamespace(time_delta=0)


def str_to_sec(s):
    if s in ['now', '', None, 'none']:
        return None
    else:
        s = s.split(':')
        try:
            return float(s[0]) * 3600 + float(s[1]) * 60 + float(s[2])
        except ValueError:
            print('Wrong time format!')
            sys.exit(1)


def read_config(path):
    with open(path, 'r') as config_file:
        return yaml.safe_load(config_file)


def load_config():
    """
    this function can reload most settings from configuration file,
    the change does not take effect immediately, but with the after next file,
    some settings cannot be changed - like resolution, aspect, or output
    """

    if stdin_args.config:
        cfg = read_config(stdin_args.config)
    elif os.path.isfile('/etc/ffplayout/ffplayout.yml'):
        cfg = read_config('/etc/ffplayout/ffplayout.yml')
    else:
        cfg = read_config('ffplayout.yml')

    if stdin_args.start:
        p_start = str_to_sec(stdin_args.start)
    else:
        p_start = str_to_sec(cfg['playlist']['day_start'])

    if p_start is None:
        p_start = get_time('full_sec')

    if stdin_args.length:
        p_length = str_to_sec(stdin_args.length)
    else:
        p_length = str_to_sec(cfg['playlist']['length'])

    _general.stop = cfg['general']['stop_on_error']
    _general.threshold = cfg['general']['stop_threshold']

    _mail.subject = cfg['mail']['subject']
    _mail.server = cfg['mail']['smpt_server']
    _mail.port = cfg['mail']['smpt_port']
    _mail.s_addr = cfg['mail']['sender_addr']
    _mail.s_pass = cfg['mail']['sender_pass']
    _mail.recip = cfg['mail']['recipient']
    _mail.level = cfg['mail']['mail_level']

    _pre.add_logo = cfg['processing']['add_logo']
    _pre.logo = cfg['processing']['logo']
    _pre.logo_scale = cfg['processing']['logo_scale']
    _pre.logo_filter = cfg['processing']['logo_filter']
    _pre.logo_opacity = cfg['processing']['logo_opacity']
    _pre.add_loudnorm = cfg['processing']['add_loudnorm']
    _pre.loud_i = cfg['processing']['loud_I']
    _pre.loud_tp = cfg['processing']['loud_TP']
    _pre.loud_lra = cfg['processing']['loud_LRA']
    _pre.output_count = cfg['processing']['output_count']

    _playlist.mode = cfg['playlist']['playlist_mode']
    _playlist.path = cfg['playlist']['path']
    _playlist.start = p_start
    _playlist.length = p_length

    _storage.path = cfg['storage']['path']
    _storage.filler = cfg['storage']['filler_clip']
    _storage.extensions = cfg['storage']['extensions']
    _storage.shuffle = cfg['storage']['shuffle']

    _text.add_text = cfg['text']['add_text']
    _text.over_pre = cfg['text']['over_pre']
    _text.address = cfg['text']['bind_address']
    _text.fontfile = cfg['text']['fontfile']
    _text.text_from_filename = cfg['text']['text_from_filename']
    _text.style = cfg['text']['style']
    _text.regex = cfg['text']['regex']

    if _init.load:
        _log.to_file = cfg['logging']['log_to_file']
        _log.backup_count = cfg['logging']['backup_count']
        _log.path = cfg['logging']['log_path']
        _log.level = cfg['logging']['log_level']
        _log.ff_level = cfg['logging']['ffmpeg_level']

        _pre.w = cfg['processing']['width']
        _pre.h = cfg['processing']['height']
        _pre.aspect = cfg['processing']['aspect']
        _pre.fps = cfg['processing']['fps']
        _pre.v_bitrate = cfg['processing']['width'] * \
            cfg['processing']['height'] / 10
        _pre.v_bufsize = _pre.v_bitrate / 2
        _pre.realtime = cfg['processing']['use_realtime']

        _playout.mode = cfg['out']['mode']
        _playout.name = cfg['out']['service_name']
        _playout.provider = cfg['out']['service_provider']
        _playout.ffmpeg_param = cfg['out']['ffmpeg_param'].split(' ')
        _playout.stream_output = cfg['out']['stream_output'].split(' ')
        _playout.hls_output = cfg['out']['hls_output'].split(' ')

        _init.load = False


load_config()


# ------------------------------------------------------------------------------
# logging
# ------------------------------------------------------------------------------

class CustomFormatter(logging.Formatter):
    """
    Logging Formatter to add colors and count warning / errors
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
        record.msg = self.format_message(record.getMessage())
        log_fmt = self.FORMATS.get(record.levelno)
        formatter = logging.Formatter(log_fmt)
        return formatter.format(record)


# If the log file is specified on the command line then override the default
if stdin_args.log:
    _log.path = stdin_args.log

playout_logger = logging.getLogger('playout')
playout_logger.setLevel(_log.level)
decoder_logger = logging.getLogger('decoder')
decoder_logger.setLevel(_log.ff_level)
encoder_logger = logging.getLogger('encoder')
encoder_logger.setLevel(_log.ff_level)

if _log.to_file and _log.path != 'none':
    if _log.path and os.path.isdir(_log.path):
        playout_log = os.path.join(_log.path, 'ffplayout.log')
        decoder_log = os.path.join(_log.path, 'decoder.log')
        encoder_log = os.path.join(_log.path, 'encoder.log')
    else:
        base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        log_dir = os.path.join(base_dir, 'log')
        os.makedirs(log_dir, exist_ok=True)
        playout_log = os.path.join(log_dir, 'ffplayout.log')
        decoder_log = os.path.join(log_dir, 'decoder.log')
        encoder_log = os.path.join(log_dir, 'encoder.log')

    p_format = logging.Formatter('[%(asctime)s] [%(levelname)s]  %(message)s')
    f_format = logging.Formatter('[%(asctime)s]  %(message)s')
    p_file_handler = TimedRotatingFileHandler(playout_log, when='midnight',
                                              backupCount=_log.backup_count)
    d_file_handler = TimedRotatingFileHandler(decoder_log, when='midnight',
                                              backupCount=_log.backup_count)
    e_file_handler = TimedRotatingFileHandler(encoder_log, when='midnight',
                                              backupCount=_log.backup_count)

    p_file_handler.setFormatter(p_format)
    d_file_handler.setFormatter(f_format)
    e_file_handler.setFormatter(f_format)
    playout_logger.addHandler(p_file_handler)
    decoder_logger.addHandler(d_file_handler)
    encoder_logger.addHandler(e_file_handler)

    DEC_PREFIX = ''
    ENC_PREFIX = ''
else:
    console_handler = logging.StreamHandler()
    console_handler.setFormatter(CustomFormatter())
    playout_logger.addHandler(console_handler)
    decoder_logger.addHandler(console_handler)
    encoder_logger.addHandler(console_handler)

    DEC_PREFIX = '[decoder] '
    ENC_PREFIX = '[encoder] '


# ------------------------------------------------------------------------------
# mail sender
# ------------------------------------------------------------------------------

class Mailer:
    """
    mailer class for sending log messages, with level selector
    """

    def __init__(self):
        self.level = _mail.level
        self.time = None
        self.timestamp = get_time('stamp')
        self.rate_limit = 600
        self.temp_msg = os.path.join(tempfile.gettempdir(), 'ffplayout.txt')

    def current_time(self):
        self.time = get_time(None)

    def send_mail(self, msg):
        if _mail.recip:
            # write message to temp file for rate limit
            with open(self.temp_msg, 'w+') as f:
                f.write(msg)

            self.current_time()

            message = MIMEMultipart()
            message['From'] = _mail.s_addr
            message['To'] = _mail.recip
            message['Subject'] = _mail.subject
            message['Date'] = formatdate(localtime=True)
            message.attach(MIMEText(f'{self.time} {msg}', 'plain'))
            text = message.as_string()

            try:
                server = smtplib.SMTP(_mail.server, _mail.port)
            except socket.error as err:
                playout_logger.error(err)
                server = None

            if server is not None:
                server.starttls()
                try:
                    login = server.login(_mail.s_addr, _mail.s_pass)
                except smtplib.SMTPAuthenticationError as serr:
                    playout_logger.error(serr)
                    login = None

                if login is not None:
                    server.sendmail(_mail.s_addr,
                                    re.split(', |; |,|;', _mail.recip), text)
                    server.quit()

    def check_if_new(self, msg):
        # send messege only when is new or the rate_limit is pass
        if os.path.isfile(self.temp_msg):
            mod_time = os.path.getmtime(self.temp_msg)

            with open(self.temp_msg, 'r', encoding='utf-8') as f:
                last_msg = f.read()

                if msg != last_msg \
                        or get_time('stamp') - mod_time > self.rate_limit:
                    self.send_mail(msg)
        else:
            self.send_mail(msg)

    def info(self, msg):
        if self.level in ['INFO']:
            self.check_if_new(msg)

    def warning(self, msg):
        if self.level in ['INFO', 'WARNING']:
            self.check_if_new(msg)

    def error(self, msg):
        if self.level in ['INFO', 'WARNING', 'ERROR']:
            self.check_if_new(msg)


class Messenger:
    """
    all logging and mail messages end up here,
    from here they go to logger and mailer
    """

    def __init__(self):
        self._mailer = Mailer()

    def debug(self, msg):
        playout_logger.debug(msg.replace('\n', ' '))

    def info(self, msg):
        playout_logger.info(msg.replace('\n', ' '))
        self._mailer.info(msg)

    def warning(self, msg):
        playout_logger.warning(msg.replace('\n', ' '))
        self._mailer.warning(msg)

    def error(self, msg):
        playout_logger.error(msg.replace('\n', ' '))
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
    if 'libx264' not in FF_LIBS['libs']:
        playout_logger.error('ffmpeg contains no libx264!')
    if 'libfdk-aac' not in FF_LIBS['libs']:
        playout_logger.warning(
            'ffmpeg contains no libfdk-aac! No high quality aac...')
    if 'tpad' not in FF_LIBS['filters']:
        playout_logger.error('ffmpeg contains no tpad filter!')
    if 'zmq' not in FF_LIBS['filters']:
        playout_logger.error(
            'ffmpeg contains no zmq filter!  Text messages will not work...')


# ------------------------------------------------------------------------------
# probe media infos
# ------------------------------------------------------------------------------

class MediaProbe:
    """
    get infos about media file, similare to mediainfo
    """

    def load(self, file):
        self.remote_source = ['http', 'https', 'ftp', 'smb', 'sftp']
        self.src = file
        self.format = None
        self.audio = []
        self.video = []

        if self.src and self.src.split('://')[0] in self.remote_source:
            url = self.src.split('://')
            self.src = f'{url[0]}://{urllib.parse.quote(url[1])}'
            self.is_remote = True
        else:
            self.is_remote = False

            if not self.src or not os.path.isfile(self.src):
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

        for stream in info['streams']:
            if stream['codec_type'] == 'audio':
                self.audio.append(stream)

            if stream['codec_type'] == 'video':
                if stream.get('display_aspect_ratio'):
                    w, h = stream['display_aspect_ratio'].split(':')
                    stream['aspect'] = float(w) / float(h)
                else:
                    stream['aspect'] = float(
                        stream['width']) / float(stream['height'])

                a, b = stream['r_frame_rate'].split('/')
                stream['fps'] = float(a) / float(b)

                self.video.append(stream)


# ------------------------------------------------------------------------------
# global helper functions
# ------------------------------------------------------------------------------

def handle_sigterm(sig, frame):
    """
    handler for ctrl+c signal
    """
    raise(SystemExit)


def handle_sighub(sig, frame):
    """
    handling SIGHUB signal for reload configuration
    Linux/macOS only
    """
    messenger.info('Reload config file')
    load_config()


signal.signal(signal.SIGTERM, handle_sigterm)

if os.name == 'posix':
    signal.signal(signal.SIGHUP, handle_sighub)


def terminate_processes(watcher=None):
    """
    kill orphaned processes
    """
    if _ff.decoder and _ff.decoder.poll() is None:
        _ff.decoder.terminate()

    if _ff.encoder and _ff.encoder.poll() is None:
        _ff.encoder.terminate()

    if watcher:
        watcher.stop()


def ffmpeg_stderr_reader(std_errors, decoder):
    """
    read fmpeg stderr decoder and encoder instance
    and log the output
    """
    if decoder:
        logger = decoder_logger
        prefix = DEC_PREFIX
    else:
        logger = encoder_logger
        prefix = ENC_PREFIX

    try:
        for line in std_errors:
            if _log.ff_level == 'INFO':
                logger.info(f'{prefix}{line.decode("utf-8").rstrip()}')
            elif _log.ff_level == 'WARNING':
                logger.warning(f'{prefix}{line.decode("utf-8").rstrip()}')
            else:
                logger.error(f'{prefix}{line.decode("utf-8").rstrip()}')
    except ValueError:
        pass


def get_date(seek_day, shift=0):
    """
    get date for correct playlist,
    when seek_day is set:
    check if playlist date must be from yesterday
    """
    d = date.today()
    if shift:
        d + timedelta(seconds=shift + _general.threshold)

    if seek_day and get_time('full_sec') < _playlist.start:
        return (d - timedelta(1)).strftime('%Y-%m-%d')
    elif _playlist.start == 0 and \
            86400 - (get_time('full_sec') + shift) < 2:
        return (d + timedelta(1)).strftime('%Y-%m-%d')
    else:
        return d.strftime('%Y-%m-%d')


def get_float(value, default=False):
    """
    test if value is float
    """
    try:
        return float(value)
    except (ValueError, TypeError):
        return default


def is_advertisement(node):
    if node and node.get('category') == 'advertisement':
        return True


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


def check_sync(delta):
    """
    check that we are in tolerance time
    """

    if _playlist.mode and _playlist.start and _playlist.length:
        # save time delta to global variable for syncing
        # this is needed for real time filter
        _global.time_delta = delta

    if _general.stop and abs(delta) > _general.threshold:
        messenger.error(
            f'Sync tolerance value exceeded with {delta:.2f} seconds,\n'
            'program terminated!')
        terminate_processes()
        sys.exit(1)


def seek_in(seek):
    """
    seek in clip
    """
    if seek > 0.0:
        return ['-ss', str(seek)]
    else:
        return []


def set_length(duration, seek, out):
    """
    set new clip length
    """
    if out < duration:
        return ['-t', str(out - seek)]
    else:
        return []


def loop_input(source, src_duration, target_duration):
    # loop filles n times
    loop_count = math.ceil(target_duration / src_duration)
    messenger.info(f'Loop "{source}" {loop_count} times, '
                   f'total duration: {target_duration:.2f}')
    return ['-stream_loop', str(loop_count),
            '-i', source, '-t', str(target_duration)]


def gen_dummy(duration):
    """
    generate a dummy clip, with black color and empty audiotrack
    """
    color = '#121212'
    # IDEA: add noise could be an config option
    # noise = 'noise=alls=50:allf=t+u,hue=s=0'
    return [
        '-f', 'lavfi', '-i',
        f'color=c={color}:s={_pre.w}x{_pre.h}:d={duration}:r={_pre.fps},'
        'format=pix_fmts=yuv420p',
        '-f', 'lavfi', '-i', f'anoisesrc=d={duration}:c=pink:r=48000:a=0.05'
    ]


def gen_filler(node):
    """
    generate filler clip to fill empty space in playlist
    """
    probe = MediaProbe()
    probe.load(_storage.filler)
    duration = node['out'] - node['seek']

    node['probe'] = probe

    if probe.format:
        if probe.format.get('duration'):
            filler_duration = float(probe.format['duration'])
            if filler_duration > duration:
                # cut filler
                messenger.info(
                    f'Generate filler with {duration:.2f} seconds')
                node['source'] = _storage.filler
                node['src_cmd'] = ['-i', _storage.filler] + set_length(
                    filler_duration, 0, duration)
                return node
            else:
                # loop file n times
                node['src_cmd'] = loop_input(_storage.filler, filler_duration,
                                             duration)
                return node
        else:
            messenger.error("Can't get filler length, generate dummy!")
            dummy = gen_dummy(duration)
            node['source'] = dummy[3]
            node['src_cmd'] = dummy
            return node

    else:
        # when no filler is set, generate a dummy
        messenger.warning('No filler is set!')
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
    elif node.get('source') and os.path.isfile(node['source']):
        if node['out'] > node['duration']:
            if node['seek'] > 0.0:
                messenger.warning(
                    f'Seek in looped source "{node["source"]}" not supported!')
                node['src_cmd'] = [
                    '-i', node['source']
                    ] + set_length(node['duration'], node['seek'],
                                   node['out'] - node['seek'])
            else:
                # FIXME: when list starts with looped clip,
                # the logo length will be wrong
                node['src_cmd'] = loop_input(node['source'], node['duration'],
                                             node['out'])
        else:
            node['src_cmd'] = seek_in(node['seek']) + \
                ['-i', node['source']] + set_length(node['duration'],
                                                    node['seek'], node['out'])
    else:
        node = gen_filler(node)

    return node


def pre_audio_codec():
    """
    when add_loudnorm is False we use a different audio encoder,
    s302m has higher quality, but is experimental
    and works not well together with the loudnorm filter
    """
    if _pre.add_loudnorm:
        return ['-c:a', 'mp2', '-b:a', '384k', '-ar', '48000', '-ac', '2']
    else:
        return ['-c:a', 's302m', '-strict', '-2', '-ar', '48000', '-ac', '2']
