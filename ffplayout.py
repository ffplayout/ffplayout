#!/usr/bin/env python3
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

import configparser
import glob
import json
import logging
import math
import os
import random
import re
import signal
import smtplib
import socket
import ssl
import sys
import tempfile
import time
from argparse import ArgumentParser
from datetime import date, datetime, timedelta
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from email.utils import formatdate
from logging.handlers import TimedRotatingFileHandler
from subprocess import PIPE, CalledProcessError, Popen, check_output
from threading import Thread
from types import SimpleNamespace
from urllib import request

try:
    if os.name != 'posix':
        import colorama
        colorama.init()

    from watchdog.events import PatternMatchingEventHandler
    from watchdog.observers import Observer
except ImportError:
    print('Some modules are not installed, ffplayout may or may not work')


# ------------------------------------------------------------------------------
# argument parsing
# ------------------------------------------------------------------------------

stdin_parser = ArgumentParser(
    description='python and ffmpeg based playout',
    epilog="don't use parameters if you want to use this settings from config")

stdin_parser.add_argument(
    '-c', '--config', help='file path to ffplayout.conf'
)

stdin_parser.add_argument(
    '-d', '--desktop', help='preview on desktop', action='store_true'
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
_pre_comp = SimpleNamespace()
_playlist = SimpleNamespace()
_storage = SimpleNamespace()
_text = SimpleNamespace()
_playout = SimpleNamespace()

_init = SimpleNamespace(load=True)
_ff = SimpleNamespace(decoder=None, encoder=None)

_WINDOWS = os.name == 'nt'
COPY_BUFSIZE = 1024 * 1024 if _WINDOWS else 64 * 1024


def load_config():
    """
    this function can reload most settings from configuration file,
    the change does not take effect immediately, but with the after next file,
    some settings cannot be changed - like resolution, aspect, or output
    """
    cfg = configparser.ConfigParser()

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

    if stdin_args.config:
        cfg.read(stdin_args.config)
    elif os.path.isfile('/etc/ffplayout/ffplayout.conf'):
        cfg.read('/etc/ffplayout/ffplayout.conf')
    else:
        cfg.read('ffplayout.conf')

    if stdin_args.start:
        p_start = str_to_sec(stdin_args.start)
    else:
        p_start = str_to_sec(cfg.get('PLAYLIST', 'day_start'))

    if not p_start:
        p_start = get_time('full_sec')

    if stdin_args.length:
        p_length = str_to_sec(stdin_args.length)
    else:
        p_length = str_to_sec(cfg.get('PLAYLIST', 'length'))

    _general.stop = cfg.getboolean('GENERAL', 'stop_on_error')
    _general.threshold = cfg.getfloat('GENERAL', 'stop_threshold')

    _mail.subject = cfg.get('MAIL', 'subject')
    _mail.server = cfg.get('MAIL', 'smpt_server')
    _mail.port = cfg.getint('MAIL', 'smpt_port')
    _mail.s_addr = cfg.get('MAIL', 'sender_addr')
    _mail.s_pass = cfg.get('MAIL', 'sender_pass')
    _mail.recip = cfg.get('MAIL', 'recipient')
    _mail.level = cfg.get('MAIL', 'mail_level')

    _pre_comp.add_logo = cfg.getboolean('PRE_COMPRESS', 'add_logo')
    _pre_comp.logo = cfg.get('PRE_COMPRESS', 'logo')
    _pre_comp.opacity = cfg.get('PRE_COMPRESS', 'logo_opacity')
    _pre_comp.logo_filter = cfg.get('PRE_COMPRESS', 'logo_filter')
    _pre_comp.add_loudnorm = cfg.getboolean('PRE_COMPRESS', 'add_loudnorm')
    _pre_comp.loud_i = cfg.getfloat('PRE_COMPRESS', 'loud_I')
    _pre_comp.loud_tp = cfg.getfloat('PRE_COMPRESS', 'loud_TP')
    _pre_comp.loud_lra = cfg.getfloat('PRE_COMPRESS', 'loud_LRA')

    _playlist.mode = cfg.getboolean('PLAYLIST', 'playlist_mode')
    _playlist.path = cfg.get('PLAYLIST', 'path')
    _playlist.start = p_start
    _playlist.length = p_length

    _storage.path = cfg.get('STORAGE', 'path')
    _storage.filler = cfg.get('STORAGE', 'filler_clip')
    _storage.extensions = json.loads(cfg.get('STORAGE', 'extensions'))
    _storage.shuffle = cfg.getboolean('STORAGE', 'shuffle')

    _text.add_text = cfg.getboolean('TEXT', 'add_text')
    _text.textfile = cfg.get('TEXT', 'textfile')
    _text.fontsize = cfg.get('TEXT', 'fontsize')
    _text.fontcolor = cfg.get('TEXT', 'fontcolor')
    _text.fontfile = cfg.get('TEXT', 'fontfile')
    _text.box = cfg.get('TEXT', 'box')
    _text.boxcolor = cfg.get('TEXT', 'boxcolor')
    _text.boxborderw = cfg.get('TEXT', 'boxborderw')
    _text.x = cfg.get('TEXT', 'x')
    _text.y = cfg.get('TEXT', 'y')

    if _init.load:
        _log.to_file = cfg.getboolean('LOGGING', 'log_to_file')
        _log.path = cfg.get('LOGGING', 'log_path')
        _log.level = cfg.get('LOGGING', 'log_level')
        _log.ff_level = cfg.get('LOGGING', 'ffmpeg_level')

        _pre_comp.w = cfg.getint('PRE_COMPRESS', 'width')
        _pre_comp.h = cfg.getint('PRE_COMPRESS', 'height')
        _pre_comp.aspect = cfg.getfloat('PRE_COMPRESS', 'aspect')
        _pre_comp.fps = cfg.getint('PRE_COMPRESS', 'fps')
        _pre_comp.v_bitrate = cfg.getint('PRE_COMPRESS', 'width') * 50
        _pre_comp.v_bufsize = cfg.getint('PRE_COMPRESS', 'width') * 50 / 2

        _playout.preview = cfg.getboolean('OUT', 'preview')
        _playout.name = cfg.get('OUT', 'service_name')
        _playout.provider = cfg.get('OUT', 'service_provider')
        _playout.out_addr = cfg.get('OUT', 'out_addr')
        _playout.post_comp_video = json.loads(
            cfg.get('OUT', 'post_comp_video'))
        _playout.post_comp_audio = json.loads(
            cfg.get('OUT', 'post_comp_audio'))
        _playout.post_comp_extra = json.loads(
            cfg.get('OUT', 'post_comp_extra'))

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
        if '"' in msg and '[' in msg:
            msg = re.sub('(".*?")', self.cyan + r'\1' + self.reset, msg)
        elif '[decoder]' in msg:
            msg = re.sub(r'(\[decoder\])', self.reset + r'\1', msg)
        elif '[encoder]' in msg:
            msg = re.sub(r'(\[encoder\])', self.reset + r'\1', msg)
        elif '/' in msg or '\\' in msg:
            msg = re.sub(
                r'(["\w.:/]+/|["\w.:]+\\.*?)', self.magenta + r'\1', msg)
        elif re.search(r'\d', msg):
            msg = re.sub(
                '([0-9.:-]+)', self.yellow + r'\1' + self.reset, msg)

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
        playout_log = os.path.join(os.getcwd(), 'ffplayout.log')
        decoder_log = os.path.join(os.getcwd(), 'ffdecoder.log')
        encoder_log = os.path.join(os.getcwd(), 'ffencoder.log')

    p_format = logging.Formatter('[%(asctime)s] [%(levelname)s]  %(message)s')
    f_format = logging.Formatter('[%(asctime)s]  %(message)s')
    p_file_handler = TimedRotatingFileHandler(playout_log, when='midnight',
                                              backupCount=5)
    d_file_handler = TimedRotatingFileHandler(decoder_log, when='midnight',
                                              backupCount=5)
    e_file_handler = TimedRotatingFileHandler(encoder_log, when='midnight',
                                              backupCount=5)

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
            message.attach(MIMEText('{} {}'.format(self.time, msg), 'plain'))
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
                    server.sendmail(_mail.s_addr, _mail.recip, text)
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
# check ffmpeg libs
# ------------------------------------------------------------------------------

def ffmpeg_libs():
    """
    check which external libs are compiled in ffmpeg,
    for using them later
    """
    cmd = ['ffmpeg', '-version']
    libs = []

    try:
        info = check_output(cmd).decode('UTF-8')
    except CalledProcessError as err:
        messenger.error('ffmpeg - libs could not be readed!\n'
                        'Processing is not possible. Error:\n{}'.format(err))
        sys.exit(1)

    for line in info.split('\n'):
        if 'configuration:' in line:
            configs = line.split()

            for cfg in configs:
                if '--enable-lib' in cfg:
                    libs.append(cfg.replace('--enable-', ''))
            break

    return libs


FF_LIBS = ffmpeg_libs()


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
            messenger.error('MediaProbe error in: "{}"\n {}'.format(self.src,
                                                                    err))
            self.audio.append(None)
            self.video.append(None)

            return

        self.format = info['format']

        for stream in info['streams']:
            if stream['codec_type'] == 'audio':
                self.audio.append(stream)

            if stream['codec_type'] == 'video':
                if 'display_aspect_ratio' not in stream:
                    stream['aspect'] = float(
                        stream['width']) / float(stream['height'])
                else:
                    w, h = stream['display_aspect_ratio'].split(':')
                    stream['aspect'] = float(w) / float(h)

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


def ffmpeg_stderr_reader(std_errors, logger, prefix):
    try:
        for line in std_errors:
            if _log.ff_level == 'INFO':
                logger.info('{}{}'.format(
                    prefix, line.decode("utf-8").rstrip()))
            elif _log.ff_level == 'WARNING':
                logger.warning('{}{}'.format(
                    prefix, line.decode("utf-8").rstrip()))
            else:
                logger.error('{}{}'.format(
                    prefix, line.decode("utf-8").rstrip()))
    except ValueError:
        pass


def get_date(seek_day):
    """
    get date for correct playlist,
    when seek_day is set:
    check if playlist date must be from yesterday
    """
    d = date.today()
    if seek_day and get_time('full_sec') < _playlist.start:
        yesterday = d - timedelta(1)
        return yesterday.strftime('%Y-%m-%d')
    else:
        return d.strftime('%Y-%m-%d')


def is_float(value):
    """
    test if value is float
    """
    try:
        float(value)
        return True
    except (ValueError, TypeError):
        return False


def is_int(value):
    """
    test if value is int
    """
    try:
        int(value)
        return True
    except ValueError:
        return False


def valid_json(file):
    """
    simple json validation
    """
    try:
        json_object = json.load(file)
        return json_object
    except ValueError:
        messenger.error("Playlist {} is not JSON conform".format(file))
        return None


def check_sync(delta):
    """
    check that we are in tolerance time
    """
    if _general.stop and abs(delta) > _general.threshold:
        messenger.error(
            'Sync tolerance value exceeded with {0:.2f} seconds,\n'
            'program terminated!'.format(delta))
        terminate_processes()
        sys.exit(1)


def check_length(total_play_time):
    """
    check if playlist is long enough
    """
    if _playlist.length and total_play_time < _playlist.length - 5 \
            and not stdin_args.loop:
        messenger.error(
            'Playlist ({}) is not long enough!\n'
            'Total play time is: {}, target length is: {}'.format(
                get_date(True),
                timedelta(seconds=total_play_time),
                timedelta(seconds=_playlist.length))
        )


def validate_thread(clip_nodes):
    """
    validate json values in new thread
    and test if source paths exist
    """
    def check_json(json_nodes):
        error = ''
        counter = 0
        probe = MediaProbe()

        # check if all values are valid
        for node in json_nodes["program"]:
            source = node["source"]
            probe.load(source)
            missing = []

            if probe.is_remote:
                if not probe.video[0]:
                    missing.append('Stream not exist: "{}"'.format(source))
            elif not os.path.isfile(source):
                missing.append('File not exist: "{}"'.format(source))

            if is_float(node["in"]) and is_float(node["out"]):
                counter += node["out"] - node["in"]
            else:
                missing.append('Missing Value in: "{}"'.format(node))

            if not is_float(node["duration"]):
                missing.append('No duration Value!')

            line = '\n'.join(missing)
            if line:
                error += line + '\nIn line: {}\n\n'.format(node)

        if error:
            messenger.error(
                'Validation error, check JSON playlist, '
                'values are missing:\n{}'.format(error)
            )

        check_length(counter)

    validate = Thread(name='check_json', target=check_json, args=(clip_nodes,))
    validate.daemon = True
    validate.start()


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
    messenger.info(
        'Loop "{0}" {1} times, total duration: {2:.2f}'.format(
            source, loop_count, target_duration))
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
        'color=c={}:s={}x{}:d={}:r={},format=pix_fmts=yuv420p'.format(
            color, _pre_comp.w, _pre_comp.h, duration, _pre_comp.fps
        ),
        '-f', 'lavfi', '-i', 'anoisesrc=d={}:c=pink:r=48000:a=0.05'.format(
            duration)
    ]


def gen_filler(duration):
    """
    when playlist is not 24 hours long, we generate a loop from filler clip
    """
    probe = MediaProbe()
    probe.load(_storage.filler)

    if probe.format:
        if 'duration' in probe.format:
            filler_duration = float(probe.format['duration'])
            if filler_duration > duration:
                # cut filler
                messenger.info(
                    'Generate filler with {0:.2f} seconds'.format(duration))
                return probe, ['-i', _storage.filler] + set_length(
                    filler_duration, 0, duration)
            else:
                # loop file n times
                return probe, loop_input(_storage.filler,
                                         filler_duration, duration)
        else:
            messenger.error("Can't get filler length, generate dummy!")
            return probe, gen_dummy(duration)

    else:
        # when no filler is set, generate a dummy
        messenger.warning('No filler is set!')
        return probe, gen_dummy(duration)


def src_or_dummy(probe, src, dur, seek, out):
    """
    when source path exist, generate input with seek and out time
    when path not exist, generate dummy clip
    """

    # check if input is a remote source
    if probe.is_remote and probe.video[0]:
        if seek > 0.0:
            messenger.warning(
                'Seek in live source "{}" not supported!'.format(src))
        return ['-i', src] + set_length(86400.0, seek, out)
    elif src and os.path.isfile(src):
        if out > dur:
            if seek > 0.0:
                messenger.warning(
                    'Seek in looped source "{}" not supported!'.format(src))
                return ['-i', src] + set_length(dur, seek, out - seek)
            else:
                # FIXME: when list starts with looped clip,
                # the logo length will be wrong
                return loop_input(src, dur, out)
        else:
            return seek_in(seek) + ['-i', src] + set_length(dur, seek, out)
    else:
        messenger.error('Clip/URL not exist:\n{}'.format(src))
        return gen_dummy(out - seek)


def get_delta(begin):
    """
    get difference between current time and begin from clip in playlist
    """
    current_time = get_time('full_sec')

    if _playlist.length:
        target_playtime = _playlist.length
    else:
        target_playtime = 86400.0

    if _playlist.start >= current_time and not begin == _playlist.start:
        current_time += target_playtime

    current_delta = begin - current_time

    if math.isclose(current_delta, 86400.0, abs_tol=6):
        current_delta -= 86400.0

    ref_time = target_playtime + _playlist.start
    total_delta = ref_time - begin + current_delta

    return current_delta, total_delta


def handle_list_init(current_delta, total_delta, seek, out):
    """
    # handle init clip, but this clip can be the last one in playlist,
    # this we have to figure out and calculate the right length
    """
    new_seek = abs(current_delta) + seek
    new_out = out

    if 1 > new_seek:
        new_seek = 0

    if out - new_seek > total_delta:
        new_out = total_delta + new_seek

    if total_delta > new_out - new_seek > 1:
        return new_seek, new_out, False

    elif new_out - new_seek > 1:
        return new_seek, new_out, True
    else:
        return 0, 0, True


def handle_list_end(probe, new_length, src, begin, dur, seek, out):
    """
    when we come to last clip in playlist,
    or when we reached total playtime,
    we end up here
    """
    new_out = out
    new_playlist = True

    if seek > 0:
        new_out = seek + new_length
    else:
        new_out = new_length
    # prevent looping
    if new_out > dur:
        new_out = dur
    else:
        messenger.info(
            'We are over time, new length is: {0:.2f}'.format(new_length))

    if dur > new_length > 1.5:
        src_cmd = src_or_dummy(probe, src, dur, seek, new_out)
    elif dur > new_length > 0.0:
        messenger.info(
            'Last clip less then 1.5 second long, skip:\n{}'.format(src))
        src_cmd = None
    else:
        missing_secs = abs(new_length - dur)
        new_out = out
        new_playlist = False
        src_cmd = src_or_dummy(probe, src, dur, seek, out)
        messenger.error(
            'Playlist is not long enough:'
            '\n{0:.2f} seconds needed.'.format(missing_secs))

    return src_cmd, seek, new_out, new_playlist


def timed_source(probe, src, begin, dur, seek, out, first, last):
    """
    prepare input clip
    check begin and length from clip
    return clip only if we are in 24 hours time range
    """
    current_delta, total_delta = get_delta(begin)

    if first:
        _seek, _out, new_list = handle_list_init(current_delta, total_delta,
                                                 seek, out)
        if _out > 1.0:
            return src_or_dummy(probe, src, dur, _seek, _out), \
                _seek, _out, new_list
        else:
            messenger.warning('Clip less then a second, skip:\n{}'.format(src))
            return None, 0, 0, True

    else:
        if not stdin_args.loop and _playlist.length:
            check_sync(current_delta)
            messenger.debug('current_delta: {:f}'.format(current_delta))
            messenger.debug('total_delta: {:f}'.format(total_delta))

        if (total_delta > out - seek and not last) \
                or stdin_args.loop or not _playlist.length:
            # when we are in the 24 houre range, get the clip
            return src_or_dummy(probe, src, dur, seek, out), seek, out, False

        elif total_delta <= 0:
            messenger.info(
                'Start time is over playtime, skip clip:\n{}'.format(src))
            return None, 0, 0, True

        elif total_delta < out - seek or last:
            return handle_list_end(probe, total_delta, src,
                                   begin, dur, seek, out)

        else:
            return None, 0, 0, True


def pre_audio_codec():
    """
    when add_loudnorm is False we use a different audio encoder,
    s302m has higher quality, but is experimental
    and works not well together with the loudnorm filter
    """
    if _pre_comp.add_loudnorm:
        acodec = 'libtwolame' if 'libtwolame' in FF_LIBS else 'mp2'
        audio = ['-c:a', acodec, '-b:a', '384k', '-ar', '48000', '-ac', '2']
    else:
        audio = ['-c:a', 's302m', '-strict', '-2', '-ar', '48000', '-ac', '2']

    return audio


# ------------------------------------------------------------------------------
# building filters,
# when is needed add individuell filters to match output format
# ------------------------------------------------------------------------------

def deinterlace_filter(probe):
    """
    when material is interlaced,
    set deinterlacing filter
    """
    filter_chain = []

    if 'field_order' in probe.video[0] and \
            probe.video[0]['field_order'] != 'progressive':
        filter_chain.append('yadif=0:-1:0')

    return filter_chain


def pad_filter(probe):
    """
    if source and target aspect is different,
    fix it with pillarbox or letterbox
    """
    filter_chain = []

    if not math.isclose(probe.video[0]['aspect'],
                        _pre_comp.aspect, abs_tol=0.03):
        if probe.video[0]['aspect'] < _pre_comp.aspect:
            filter_chain.append(
                'pad=ih*{}/{}/sar:ih:(ow-iw)/2:(oh-ih)/2'.format(_pre_comp.w,
                                                                 _pre_comp.h))
        elif probe.video[0]['aspect'] > _pre_comp.aspect:
            filter_chain.append(
                'pad=iw:iw*{}/{}/sar:(ow-iw)/2:(oh-ih)/2'.format(_pre_comp.h,
                                                                 _pre_comp.w))

    return filter_chain


def fps_filter(probe):
    """
    changing frame rate
    """
    filter_chain = []

    if probe.video[0]['fps'] != _pre_comp.fps:
        filter_chain.append('framerate=fps={}'.format(_pre_comp.fps))

    return filter_chain


def scale_filter(probe):
    """
    if target resolution is different to source add scale filter,
    apply also an aspect filter, when is different
    """
    filter_chain = []

    if int(probe.video[0]['width']) != _pre_comp.w or \
            int(probe.video[0]['height']) != _pre_comp.h:
        filter_chain.append('scale={}:{}'.format(_pre_comp.w, _pre_comp.h))

    if not math.isclose(probe.video[0]['aspect'],
                        _pre_comp.aspect, abs_tol=0.03):
        filter_chain.append('setdar=dar={}'.format(_pre_comp.aspect))

    return filter_chain


def fade_filter(duration, seek, out, track=''):
    """
    fade in/out video, when is cutted at the begin or end
    """
    filter_chain = []

    if seek > 0.0:
        filter_chain.append('{}fade=in:st=0:d=0.5'.format(track))

    if out != duration:
        filter_chain.append('{}fade=out:st={}:d=1.0'.format(track,
                                                            out - seek - 1.0))

    return filter_chain


def overlay_filter(duration, ad, ad_last, ad_next):
    """
    overlay logo: when is an ad don't overlay,
    when ad is comming next fade logo out,
    when clip before was an ad fade logo in
    """
    logo_filter = '[v]null[logo]'

    if _pre_comp.add_logo and os.path.isfile(_pre_comp.logo) and not ad:
        logo_chain = []
        opacity = 'format=rgba,colorchannelmixer=aa={}'.format(
            _pre_comp.opacity)
        loop = 'loop=loop={}:size=1:start=0'.format(
                duration * _pre_comp.fps)
        logo_chain.append('movie={},{},{}'.format(
                _pre_comp.logo, loop, opacity))
        if ad_last:
            logo_chain.append('fade=in:st=0:d=1.0:alpha=1')
        if ad_next:
            logo_chain.append('fade=out:st={}:d=1.0:alpha=1'.format(
                duration - 1))

        logo_filter = '{}[l];[v][l]{}[logo]'.format(
            ','.join(logo_chain), _pre_comp.logo_filter)

    return logo_filter


def add_audio(probe, duration):
    """
    when clip has no audio we generate an audio line
    """
    line = []

    if not probe.audio:
        messenger.warning('Clip "{}" has no audio!'.format(probe.src))
        line = [
            'aevalsrc=0:channel_layout=2:duration={}:sample_rate={}'.format(
                duration, 48000)]

    return line


def add_loudnorm(probe):
    """
    add single pass loudnorm filter to audio line
    """
    loud_filter = []

    if probe.audio and _pre_comp.add_loudnorm:
        loud_filter = [('loudnorm=I={}:TP={}:LRA={}').format(
            _pre_comp.loud_i, _pre_comp.loud_tp, _pre_comp.loud_lra)]

    return loud_filter


def extend_audio(probe, duration):
    """
    check audio duration, is it shorter then clip duration - pad it
    """
    pad_filter = []

    if probe.audio and 'duration' in probe.audio[0] and \
            duration > float(probe.audio[0]['duration']) + 0.3:
        pad_filter.append('apad=whole_dur={}'.format(duration))

    return pad_filter


def extend_video(probe, duration, target_duration):
    """
    check video duration, is it shorter then clip duration - pad it
    """
    pad_filter = []

    if 'duration' in probe.video[0] and \
        target_duration < duration > float(
            probe.video[0]['duration']) + 0.3:
        pad_filter.append('tpad=stop_mode=add:stop_duration={}'.format(
            duration - float(probe.video[0]['duration'])))

    return pad_filter


def build_filtergraph(duration, seek, out, ad, ad_last, ad_next, probe):
    """
    build final filter graph, with video and audio chain
    """
    video_chain = []
    audio_chain = []
    video_map = ['-map', '[logo]']

    if out > duration:
        seek = 0

    if probe.video[0]:
        video_chain += deinterlace_filter(probe)
        video_chain += pad_filter(probe)
        video_chain += fps_filter(probe)
        video_chain += scale_filter(probe)
        video_chain += extend_video(probe, duration, out - seek)
        video_chain += fade_filter(duration, seek, out)

        audio_chain += add_audio(probe, out - seek)

        if not audio_chain:
            audio_chain.append('[0:a]anull')
            audio_chain += add_loudnorm(probe)
            audio_chain += extend_audio(probe, out - seek)
            audio_chain += fade_filter(duration, seek, out, 'a')

    if video_chain:
        video_filter = '{}[v]'.format(','.join(video_chain))
    else:
        video_filter = 'null[v]'

    logo_filter = overlay_filter(out - seek, ad, ad_last, ad_next)
    video_filter = [
        '-filter_complex', '[0:v]{};{}'.format(
            video_filter, logo_filter)]

    if audio_chain:
        audio_filter = [
            '-filter_complex', '{}[a]'.format(','.join(audio_chain))]
        audio_map = ['-map', '[a]']
    else:
        audio_filter = []
        audio_map = ['-map', '0:a']

    if probe.video[0]:
        return video_filter + audio_filter + video_map + audio_map
    else:
        return video_filter + video_map + ['-map', '1:a']


# ------------------------------------------------------------------------------
# folder watcher
# ------------------------------------------------------------------------------

class MediaStore:
    """
    fill media list for playing
    MediaWatch will interact with add and remove
    """

    def __init__(self):
        self.store = []

        if stdin_args.folder:
            self.folder = stdin_args.folder
        else:
            self.folder = _storage.path

        self.fill()

    def fill(self):
        for ext in _storage.extensions:
            self.store.extend(
                glob.glob(os.path.join(self.folder, '**', ext),
                          recursive=True))

        self.sort()

    def add(self, file):
        self.store.append(file)
        self.sort()

    def remove(self, file):
        self.store.remove(file)
        self.sort()

    def sort(self):
        # sort list for sorted playing
        self.store = sorted(self.store)


class MediaWatcher:
    """
    watch given folder for file changes and update media list
    """

    def __init__(self, media):
        self._media = media

        self.event_handler = PatternMatchingEventHandler(
            patterns=_storage.extensions)
        self.event_handler.on_created = self.on_created
        self.event_handler.on_moved = self.on_moved
        self.event_handler.on_deleted = self.on_deleted

        self.observer = Observer()
        self.observer.schedule(self.event_handler, self._media.folder,
                               recursive=True)

        self.observer.start()

    def on_created(self, event):
        # add file to media list only if it is completely copied
        file_size = -1
        while file_size != os.path.getsize(event.src_path):
            file_size = os.path.getsize(event.src_path)
            time.sleep(1)

        self._media.add(event.src_path)

        messenger.info('Add file to media list: "{}"'.format(event.src_path))

    def on_moved(self, event):
        self._media.remove(event.src_path)
        self._media.add(event.dest_path)

        messenger.info('Move file from "{}" to "{}"'.format(event.src_path,
                                                            event.dest_path))

    def on_deleted(self, event):
        self._media.remove(event.src_path)

        messenger.info(
            'Remove file from media list: "{}"'.format(event.src_path))

    def stop(self):
        self.observer.stop()
        self.observer.join()


class GetSourceFromFolder:
    """
    give next clip, depending on shuffle mode
    """

    def __init__(self, media):
        self._media = media

        self.last_played = []
        self.index = 0
        self.probe = MediaProbe()

    def next(self):
        while True:
            if _storage.shuffle:
                clip = random.choice(self._media.store)

                if len(self.last_played) > len(self._media.store) / 2:
                    self.last_played.pop(0)

                if clip not in self.last_played:
                    self.last_played.append(clip)
                    self.probe.load(clip)
                    filtergraph = build_filtergraph(
                        float(self.probe.format['duration']), 0.0,
                        float(self.probe.format['duration']), False, False,
                        False, self.probe)

                    yield ['-i', clip] + filtergraph

            else:
                while self.index < len(self._media.store):
                    self.probe.load(self._media.store[self.index])
                    filtergraph = build_filtergraph(
                        float(self.probe.format['duration']), 0.0,
                        float(self.probe.format['duration']), False, False,
                        False, self.probe)

                    yield [
                        '-i', self._media.store[self.index]
                        ] + filtergraph
                    self.index += 1
                else:
                    self.index = 0


# ------------------------------------------------------------------------------
# main functions
# ------------------------------------------------------------------------------

class GetSourceFromPlaylist:
    """
    read values from json playlist,
    get current clip in time,
    set ffmpeg source command
    """

    def __init__(self):
        self.init_time = _playlist.start
        self.last_time = get_time('full_sec')

        if _playlist.length:
            self.total_playtime = _playlist.length
        else:
            self.total_playtime = 86400.0

        if self.last_time < _playlist.start:
            self.last_time += self.total_playtime

        self.last_mod_time = 0.0
        self.json_file = None
        self.clip_nodes = None
        self.src_cmd = None
        self.probe = MediaProbe()
        self.filtergraph = []
        self.first = True
        self.last = False
        self.list_date = get_date(True)

        self.src = None
        self.begin = 0
        self.seek = 0
        self.out = 20
        self.duration = 20
        self.ad = False
        self.ad_last = False
        self.ad_next = False

    def get_playlist(self):
        if stdin_args.playlist:
            self.json_file = stdin_args.playlist
        else:
            year, month, day = self.list_date.split('-')
            self.json_file = os.path.join(
             _playlist.path, year, month, self.list_date + '.json')

        if '://' in self.json_file:
            self.json_file = self.json_file.replace('\\', '/')

            try:
                req = request.urlopen(self.json_file,
                                      timeout=1,
                                      context=ssl._create_unverified_context())
                b_time = req.headers['last-modified']
                temp_time = time.strptime(b_time, "%a, %d %b %Y %H:%M:%S %Z")
                mod_time = time.mktime(temp_time)

                if mod_time > self.last_mod_time:
                    self.clip_nodes = valid_json(req)
                    self.last_mod_time = mod_time
                    messenger.info('Open: ' + self.json_file)
                    validate_thread(self.clip_nodes)
            except (request.URLError, socket.timeout):
                self.eof_handling('Get playlist from url failed!', False)

        elif os.path.isfile(self.json_file):
            # check last modification from playlist
            mod_time = os.path.getmtime(self.json_file)
            if mod_time > self.last_mod_time:
                with open(self.json_file, 'r', encoding='utf-8') as f:
                    self.clip_nodes = valid_json(f)

                self.last_mod_time = mod_time
                messenger.info('Open: ' + self.json_file)
                validate_thread(self.clip_nodes)
        else:
            # when we have no playlist for the current day,
            # then we generate a black clip
            # and calculate the seek in time, for when the playlist comes back
            self.eof_handling('Playlist not exist:', False)

    def get_clip_in_out(self, node):
        if is_float(node["in"]):
            self.seek = node["in"]
        else:
            self.seek = 0

        if is_float(node["duration"]):
            self.duration = node["duration"]
        else:
            self.duration = 20

        if is_float(node["out"]):
            self.out = node["out"]
        else:
            self.out = self.duration

    def get_input(self):
        self.src_cmd, self.seek, self.out, self.next_playlist = timed_source(
            self.probe, self.src, self.begin, self.duration,
            self.seek, self.out, self.first, self.last
        )

    def get_category(self, index, node):
        if 'category' in node:
            if index - 1 >= 0:
                last_category = self.clip_nodes[
                    "program"][index - 1]["category"]
            else:
                last_category = 'noad'

            if index + 2 <= len(self.clip_nodes["program"]):
                next_category = self.clip_nodes[
                    "program"][index + 1]["category"]
            else:
                next_category = 'noad'

            if node["category"] == 'advertisement':
                self.ad = True
            else:
                self.ad = False

            if last_category == 'advertisement':
                self.ad_last = True
            else:
                self.ad_last = False

            if next_category == 'advertisement':
                self.ad_next = True
            else:
                self.ad_next = False

    def set_filtergraph(self):
        self.filtergraph = build_filtergraph(
            self.duration, self.seek, self.out, self.ad, self.ad_last,
            self.ad_next, self.probe)

    def check_for_next_playlist(self):
        if not self.next_playlist:
            # normal behavior, when no new playlist is needed
            self.last_time = self.begin
        elif self.next_playlist and _playlist.length != 86400.0:
            # get sure that no new clip will be loaded
            self.last_time = 86400.0 * 2
        else:
            # when there is no time left and we are in time,
            # set right values for new playlist
            self.list_date = get_date(False)
            self.last_mod_time = 0.0
            self.last_time = _playlist.start - 1

    def eof_handling(self, message, fill):
        self.seek = 0.0
        self.ad = False

        current_delta, total_delta = get_delta(self.begin)

        self.out = abs(total_delta)
        self.duration = abs(total_delta) + 1
        self.list_date = get_date(False)
        self.last_mod_time = 0.0
        self.first = False
        self.last_time = 0.0

        if self.duration > 2 and fill:
            self.probe, self.src_cmd = gen_filler(self.duration)
            self.set_filtergraph()

        else:
            self.src_cmd = None
            self.next_playlist = True

        self.last = False

    def peperation_task(self, index, node):
        # call functions in order to prepare source and filter
        self.src = node["source"]
        self.probe.load(self.src)

        self.get_input()
        self.get_category(index, node)
        self.set_filtergraph()
        self.check_for_next_playlist()

    def next(self):
        while True:
            self.get_playlist()

            if self.clip_nodes is None:
                self.eof_handling('Playlist is empty!', True)
                yield self.src_cmd + self.filtergraph
                continue

            self.begin = self.init_time

            # loop through all clips in playlist and get correct clip in time
            for index, node in enumerate(self.clip_nodes["program"]):
                self.get_clip_in_out(node)

                # first time we end up here
                if self.first and \
                        self.last_time < self.begin + self.out - self.seek:

                    self.peperation_task(index, node)
                    self.first = False
                    break
                elif self.last_time < self.begin:
                    if index + 1 == len(self.clip_nodes["program"]):
                        self.last = True
                    else:
                        self.last = False

                    self.peperation_task(index, node)
                    break

                self.begin += self.out - self.seek
            else:
                if stdin_args.loop:
                    self.check_for_next_playlist()
                    self.init_time = self.last_time + 1
                    self.src_cmd = None
                elif not _playlist.length and not stdin_args.loop:
                    # when we reach playlist end, stop script
                    messenger.info('Playlist reached end!')
                    return None
                elif self.begin == self.init_time:
                    # no clip was played, generate dummy
                    self.eof_handling('Playlist is empty!', False)
                else:
                    # playlist is not long enough, play filler
                    self.eof_handling('Playlist is not long enough!', True)

            if self.src_cmd is not None:
                yield self.src_cmd + self.filtergraph


def main():
    """
    pipe ffmpeg pre-process to final ffmpeg post-process,
    or play with ffplay
    """
    year = get_date(False).split('-')[0]
    overlay = []

    ff_pre_settings = [
        '-pix_fmt', 'yuv420p', '-r', str(_pre_comp.fps),
        '-c:v', 'mpeg2video', '-intra',
        '-b:v', '{}k'.format(_pre_comp.v_bitrate),
        '-minrate', '{}k'.format(_pre_comp.v_bitrate),
        '-maxrate', '{}k'.format(_pre_comp.v_bitrate),
        '-bufsize', '{}k'.format(_pre_comp.v_bufsize)
        ] + pre_audio_codec() + ['-f', 'mpegts', '-']

    if _text.add_text and os.path.isfile(_text.textfile):
        messenger.info('Overlay text file: "{}"'.format(_text.textfile))
        overlay = [
            '-vf', ("drawtext=box={}:boxcolor='{}':boxborderw={}"
                    ":fontsize={}:fontcolor={}:fontfile='{}':textfile={}"
                    ":reload=1:x='{}':y='{}'").format(
                        _text.box, _text.boxcolor, _text.boxborderw,
                        _text.fontsize, _text.fontcolor, _text.fontfile,
                        _text.textfile, _text.x, _text.y)
        ]

    try:
        if _playout.preview or stdin_args.desktop:
            # preview playout to player
            _ff.encoder = Popen([
                'ffplay', '-hide_banner', '-nostats', '-i', 'pipe:0'
                ] + overlay, stderr=PIPE, stdin=PIPE, stdout=None)
        else:
            _ff.encoder = Popen([
                'ffmpeg', '-v', _log.ff_level.lower(), '-hide_banner',
                '-nostats', '-re', '-thread_queue_size', '256',
                '-i', 'pipe:0'] + overlay + _playout.post_comp_video
                + _playout.post_comp_audio + [
                    '-metadata', 'service_name=' + _playout.name,
                    '-metadata', 'service_provider=' + _playout.provider,
                    '-metadata', 'year={}'.format(year)
                ] + _playout.post_comp_extra + [_playout.out_addr],
                stdin=PIPE, stderr=PIPE)

        enc_err_thread = Thread(target=ffmpeg_stderr_reader,
                                args=(_ff.encoder.stderr, encoder_logger,
                                      ENC_PREFIX))
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
            for src_cmd in get_source.next():
                messenger.debug('src_cmd: "{}"'.format(src_cmd))
                if src_cmd[0] == '-i':
                    current_file = src_cmd[1]
                else:
                    current_file = src_cmd[3]

                messenger.info('Play: "{}"'.format(current_file))

                with Popen([
                    'ffmpeg', '-v', _log.ff_level.lower(), '-hide_banner',
                    '-nostats'] + src_cmd + ff_pre_settings,
                        stdout=PIPE, stderr=PIPE) as _ff.decoder:

                    dec_err_thread = Thread(target=ffmpeg_stderr_reader,
                                            args=(_ff.decoder.stderr,
                                                  decoder_logger,
                                                  DEC_PREFIX))
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
        _ff.encoder.wait()


if __name__ == '__main__':
    main()
