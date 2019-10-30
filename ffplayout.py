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
import signal
import smtplib
import socket
import ssl
import sys
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
    '-p', '--playlist', help='path from playlist'
)

stdin_parser.add_argument(
    '-s', '--start',
    help='start time in "hh:mm:ss", "now" for start with first'
)

stdin_args = stdin_parser.parse_args()

# ------------------------------------------------------------------------------
# read variables from config file
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


def load_config():
    """
    this function can reload most settings from configuration file,
    the change does not take effect immediately, but with the after next file,
    some settings cannot be changed - like resolution, aspect, or output
    """
    cfg = configparser.ConfigParser()

    def validate_time(t):
        timeformat = "%H:%M:%S"
        try:
            datetime.datetime.strptime(t, timeformat)
            return True
        except ValueError:
            messenger.error('wrong time format!')
            sys.exit(1)

    def str_to_sec(s):
        return float(s[0]) * 3600 + float(s[1]) * 60 + float(s[2])

    if stdin_args.config:
        cfg.read(stdin_args.config)
    elif os.path.isfile('/etc/ffplayout/ffplayout.conf'):
        cfg.read('/etc/ffplayout/ffplayout.conf')
    else:
        cfg.read('ffplayout.conf')

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
    _pre_comp.protocols = cfg.get('PRE_COMPRESS', 'live_protocols')

    if stdin_args.start:
        if stdin_args.start == 'now':
            start_t = None
        elif validate_time:
            stime = stdin_args.start.split(':')
            start_t = str_to_sec(stime)
    else:
        stime = cfg.get('PLAYLIST', 'day_start').split(':')

        if stime[0] and stime[1] and stime[2]:
            start_t = str_to_sec(stime)
        else:
            start_t = None

    _playlist.mode = cfg.getboolean('PLAYLIST', 'playlist_mode')
    _playlist.path = cfg.get('PLAYLIST', 'path')
    _playlist.start = start_t

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
        _log.path = cfg.get('LOGGING', 'log_file')
        _log.level = cfg.get('LOGGING', 'log_level')

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

# If the log file is specified on the command line then override the default
if stdin_args.log:
    _log.path = stdin_args.log

logger = logging.getLogger(__name__)
logger.setLevel(_log.level)
handler = TimedRotatingFileHandler(_log.path, when='midnight', backupCount=5)
formatter = logging.Formatter('[%(asctime)s] [%(levelname)s]  %(message)s')
handler.setFormatter(formatter)

if _log.to_file:
    logger.addHandler(handler)
else:
    logger.addHandler(logging.StreamHandler())


class PlayoutLogger(object):
    """
    capture stdout and sterr in the log
    """

    def __init__(self, logger, level):
        self.logger = logger
        self.level = level

    def write(self, message):
        # Only log if there is a message (not just a new line)
        if message.rstrip() != '':
            self.logger.log(self.level, message.rstrip())

    def flush(self):
        pass


# Replace stdout with logging to file at INFO level
sys.stdout = PlayoutLogger(logger, logging.INFO)
# Replace stderr with logging to file at ERROR level
sys.stderr = PlayoutLogger(logger, logging.ERROR)


# ------------------------------------------------------------------------------
# mail sender
# ------------------------------------------------------------------------------

class Mailer:
    """
    mailer class for log messages, with level selector
    """

    def __init__(self):
        self.level = _mail.level
        self.time = None

    def current_time(self):
        self.time = get_time(None)

    def send_mail(self, msg):
        if _mail.recip:
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
                logger.error(err)
                server = None

            if server is not None:
                server.starttls()
                try:
                    login = server.login(_mail.s_addr, _mail.s_pass)
                except smtplib.SMTPAuthenticationError as serr:
                    logger.error(serr)
                    login = None

                if login is not None:
                    server.sendmail(_mail.s_addr, _mail.recip, text)
                    server.quit()

    def info(self, msg):
        if self.level in ['INFO']:
            self.send_mail(msg)

    def warning(self, msg):
        if self.level in ['INFO', 'WARNING']:
            self.send_mail(msg)

    def error(self, msg):
        if self.level in ['INFO', 'WARNING', 'ERROR']:
            self.send_mail(msg)


class Messenger:
    """
    all logging and mail messages end up here,
    from here they go to logger and mailer
    """

    def __init__(self):
        self._mailer = Mailer()

    def debug(self, msg):
        logger.debug(msg.replace('\n', ' '))

    def info(self, msg):
        logger.info(msg.replace('\n', ' '))
        self._mailer.info(msg)

    def warning(self, msg):
        logger.warning(msg.replace('\n', ' '))
        self._mailer.warning(msg)

    def error(self, msg):
        logger.error(msg.replace('\n', ' '))
        self._mailer.error(msg)


messenger = Messenger()


# ------------------------------------------------------------------------------
# probe media infos
# ------------------------------------------------------------------------------

class MediaProbe:
    """
    get infos about media file, similare to mediainfo
    """

    def load(self, file):
        self.src = file
        self.format = None
        self.audio = []
        self.video = []

        cmd = ['ffprobe', '-v', 'quiet', '-print_format',
               'json', '-show_format', '-show_streams', self.src]

        info = json.loads(check_output(cmd).decode(encoding='UTF-8'))

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
    messenger.info('reload config file')
    load_config()


signal.signal(signal.SIGTERM, handle_sigterm)

if os.name == 'posix':
    signal.signal(signal.SIGHUP, handle_sighub)


def terminate_processes(decoder, encoder, watcher):
    """
    kill orphaned processes
    """
    if decoder.poll() is None:
        decoder.terminate()

    if encoder.poll() is None:
        encoder.terminate()

    if watcher:
        watcher.stop()


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


def get_date(seek_day):
    """
    get date for correct playlist,
    when _playlist.start and seek_day is set:
    check if playlist date must be from yesterday
    """
    d = date.today()
    if _playlist.start and seek_day and get_time('full_sec') < _playlist.start:
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


def check_sync(begin, encoder):
    """
    compare clip play time with real time,
    to see if we are sync
    """
    time_now = get_time('full_sec')

    time_distance = begin - time_now
    if _playlist.start and 0 <= time_now < _playlist.start and \
            not begin == _playlist.start:
        time_distance -= 86400.0

    # check that we are in tolerance time
    if _general.stop and abs(time_distance) > _general.threshold:
        messenger.error(
            'Sync tolerance value exceeded with {0:.2f} seconds,\n'
            'program terminated!'.format(time_distance))
        encoder.terminate()
        sys.exit(1)


def check_length(json_nodes, total_play_time):
    """
    check if playlist is long enough
    """
    if 'length' in json_nodes:
        l_h, l_m, l_s = json_nodes["length"].split(':')
        if is_float(l_h) and is_float(l_m) and is_float(l_s):
            length = float(l_h) * 3600 + float(l_m) * 60 + float(l_s)

            if 'date' in json_nodes:
                date = json_nodes["date"]
            else:
                date = get_date(True)

            if total_play_time < length - 5:
                messenger.error(
                    'Playlist ({}) is not long enough!\n'
                    'Total play time is: {}'.format(
                        date,
                        timedelta(seconds=total_play_time))
                )


def validate_thread(clip_nodes):
    """
    validate json values in new thread
    and test if source paths exist
    """
    def check_json(json_nodes):
        error = ''
        counter = 0

        # check if all values are valid
        for node in json_nodes["program"]:
            source = node["source"]
            prefix = source.split('://')[0]
            missing = []

            if source and prefix in _pre_comp.protocols:
                cmd = [
                    'ffprobe', '-v', 'error',
                    '-show_entries', 'format=duration',
                    '-of', 'default=noprint_wrappers=1:nokey=1', source]

                try:
                    output = check_output(cmd).decode('utf-8')
                except CalledProcessError:
                    output = '404'

                if '404' in output:
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

        check_length(json_nodes, counter)

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
    if not _storage.filler:
        # when no filler is set, generate a dummy
        messenger.warning('No filler is set!')
        return gen_dummy(duration)
    else:
        # get duration from filler
        cmd = [
            'ffprobe', '-v', 'error', '-show_entries', 'format=duration',
            '-of', 'default=noprint_wrappers=1:nokey=1', _storage.filler]

        try:
            file_duration = float(check_output(cmd).decode('utf-8'))
        except (CalledProcessError, ValueError):
            file_duration = None

        if file_duration:
            if file_duration > duration:
                # cut filler
                messenger.info(
                    'Generate filler with {0:.2f} seconds'.format(duration))
                return ['-i', _storage.filler] + set_length(
                    file_duration, 0, duration)
            else:
                # loop file n times
                return loop_input(_storage.filler, file_duration, duration)
        else:
            messenger.error("Can't get filler length, generate dummy!")
            return gen_dummy(duration)


def src_or_dummy(src, dur, seek, out):
    """
    when source path exist, generate input with seek and out time
    when path not exist, generate dummy clip
    """
    prefix = src.split('://')[0]

    # check if input is a live source
    if src and prefix in _pre_comp.protocols or os.path.isfile(src):
        if out > dur:
            return loop_input(src, dur, out)
        else:
            return seek_in(seek) + ['-i', src] + set_length(dur, seek, out)
    else:
        messenger.error('Clip not exist:\n{}'.format(src))
        return gen_dummy(out - seek)


def gen_input(src, begin, dur, seek, out, last):
    """
    prepare input clip
    check begin and length from clip
    return clip only if we are in 24 hours time range
    """

    if _playlist.start:
        ref_time = 86400.0 + _playlist.start
        new_seek = 0

        if begin + (out - seek) < ref_time and not last:
            # when we are in the 24 houre range, get the clip
            return src_or_dummy(src, dur, seek, out), out, False

        elif begin > ref_time:
            messenger.info(
                'start time is over 24 hours, skip clip:\n{}'.format(src))
            return None, 0.0, True

        elif begin + (out - seek) > ref_time:
            # when we over the 24 hours range, trim clip
            new_len = ref_time - begin

            # When calculated length from last clip is longer then 4 seconds,
            # we use the clip. When the length is less then 4 and bigger then 1
            # second we generate a black clip and when is less the a seconds
            # we skip the clip.
            if new_len > 4.0:
                messenger.info(
                    'we are over time, new_len is: {0:.2f}'.format(new_len))
                new_seek = 0

                if seek > 0:
                    new_seek = out - new_len
                    new_len = out

                src_cmd = src_or_dummy(src, dur, new_seek, new_len)
            elif new_len > 1.0:
                src_cmd = gen_dummy(new_len)
            else:
                messenger.info(
                    'last clip less then a second long, skip:\n{}'.format(src))
                src_cmd = None

            return src_cmd, new_len, True

        elif begin + (out - seek) < ref_time and last:
            # when last clip is reached and we under ref_time,
            # check if out/duration is long enough
            if begin + out > ref_time:
                new_len = ref_time - begin

                messenger.info(
                    'we are under time, new_len is: {0:.2f}'.format(
                        new_len))

                if seek > 0:
                    new_seek = out - new_len
                    new_len = out

                src_cmd = src_or_dummy(src, dur, new_seek, new_len)
                return src_cmd, new_len, True
            else:
                missing_secs = abs(begin + out - ref_time)
                messenger.error(
                    'Playlist is not long enough:'
                    '\n{0:.2f} seconds needed.'.format(missing_secs))

                src_cmd = src_or_dummy(src, dur, 0, out)
                return src_cmd, out, False

        else:
            return None, True
    else:
        return src_or_dummy(src, dur, seek, out), out, False


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
    a_samples = int(192000 / _pre_comp.fps)

    if probe.audio and _pre_comp.add_loudnorm:
        loud_filter = [('loudnorm=I={}:TP={}:LRA={},'
                        'asetnsamples=n={}').format(_pre_comp.loud_i,
                                                    _pre_comp.loud_tp,
                                                    _pre_comp.loud_lra,
                                                    a_samples)]

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
    check video duration, is is shorter then clip duration - pad it
    """
    pad_filter = []

    if 'duration' in probe.video[0] and \
        target_duration < duration > float(
            probe.video[0]['duration']) + 0.3:
        pad_filter.append('tpad=stop_mode=add:stop_duration={}'.format(
            duration - float(probe.video[0]['duration'])))

    return pad_filter


def build_filtergraph(duration, seek, out, ad, ad_last, ad_next, dummy, probe):
    """
    build final filter graph, with video and audio chain
    """
    video_chain = []
    audio_chain = []
    video_map = ['-map', '[logo]']

    if out > duration:
        seek = 0

    if not dummy:
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

    if dummy:
        return video_filter + video_map + ['-map', '1:a']
    else:
        return video_filter + audio_filter + video_map + audio_map


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


class GetSource:
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
                        False, False, self.probe)

                    yield ['-i', clip] + filtergraph

            else:
                while self.index < len(self._media.store):
                    self.probe.load(self._media.store[self.index])
                    filtergraph = build_filtergraph(
                        float(self.probe.format['duration']), 0.0,
                        float(self.probe.format['duration']), False, False,
                        False, False, self.probe)

                    yield [
                        '-i', self._media.store[self.index]
                        ] + filtergraph
                    self.index += 1
                else:
                    self.index = 0


# ------------------------------------------------------------------------------
# main functions
# ------------------------------------------------------------------------------

class GetSourceIter(object):
    """
    read values from json playlist,
    get current clip in time,
    set ffmpeg source command
    """

    def __init__(self, encoder):
        self._encoder = encoder
        self.init_time = get_time('full_sec')
        self.last_time = self.init_time
        self.day_in_sec = 86400.0

        # when _playlist.start is set, use start time
        if _playlist.start:
            self.init_time = _playlist.start

            if 0 <= self.last_time < _playlist.start:
                self.last_time += self.day_in_sec

        self.last_mod_time = 0.0
        self.json_file = None
        self.clip_nodes = None
        self.src_cmd = None
        self.probe = MediaProbe()
        self.filtergraph = []
        self.first = True
        self.last = False
        self.list_date = get_date(True)
        self.is_dummy = False
        self.last_error = ''
        self.timestamp = get_time('stamp')

        self.src = None
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
                    messenger.info('open: ' + self.json_file)
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
                messenger.info('open: ' + self.json_file)
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

    def url_or_live_source(self):
        prefix = self.src.split('://')[0]

        # check if input is a live source
        if self.src and prefix in _pre_comp.protocols:
            cmd = [
                'ffprobe', '-v', 'error', '-show_entries', 'format=duration',
                '-of', 'default=noprint_wrappers=1:nokey=1', self.src]

            try:
                output = check_output(cmd).decode('utf-8')
            except CalledProcessError as err:
                messenger.error("ffprobe error: {}".format(err))
                output = None

            if not output:
                messenger.error('Clip not exist:\n{}'.format(self.src))
                self.src = None
            elif is_float(output):
                self.duration = float(output)
            else:
                self.duration = self.day_in_sec
                self.out = self.out - self.seek
                self.seek = 0

    def get_input(self):
        self.src_cmd, self.out, self.next_playlist = gen_input(
            self.src, self.begin, self.duration,
            self.seek, self.out, self.last
        )

    def is_source_dummy(self):
        if self.src_cmd and 'lavfi' in self.src_cmd:
            self.is_dummy = True
        else:
            self.is_dummy = False

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
            self.ad_next, self.is_dummy, self.probe)

    def eof_handling(self, message, filler):
        self.seek = 0.0
        self.ad = False

        ref_time = self.day_in_sec
        time = get_time('full_sec')

        if _playlist.start:
            ref_time = self.day_in_sec + _playlist.start

            if 0 <= time < _playlist.start:
                time += self.day_in_sec

        time_diff = self.out - self.seek + time
        new_len = self.out - self.seek - (time_diff - ref_time)

        self.out = abs(new_len)
        self.duration = abs(new_len)
        self.list_date = get_date(False)
        self.last_mod_time = 0.0
        self.first = False
        self.last_time = 0.0

        if filler:
            self.src_cmd = gen_filler(self.duration)

            if _storage.filler:
                self.is_dummy = False
                self.duration += 1
                self.probe.load(_storage.filler)
            else:
                self.is_dummy = True
        else:
            self.src_cmd = gen_dummy(self.duration)
            self.is_dummy = True
        self.set_filtergraph()

        if get_time('stamp') - self.timestamp > 3600 \
                and message != self.last_error:
            self.last_error = message
            messenger.error('{}\n{}'.format(message, self.json_file))
            self.timestamp = get_time('stamp')

        messenger.error('{} {}'.format(message, self.json_file))

        self.last = False

    def next(self):
        while True:
            self.get_playlist()

            if self.clip_nodes is None:
                self.is_dummy = True
                self.set_filtergraph()
                yield self.src_cmd + self.filtergraph
                continue

            self.begin = self.init_time

            # loop through all clips in playlist
            for index, node in enumerate(self.clip_nodes["program"]):
                self.get_clip_in_out(node)

                # first time we end up here
                if self.first and \
                        self.last_time < self.begin + self.out - self.seek:
                    if _playlist.start:
                        # calculate seek time
                        self.seek = self.last_time - self.begin + self.seek

                    self.src = node["source"]
                    self.probe.load(self.src)

                    self.url_or_live_source()
                    self.get_input()
                    self.is_source_dummy()
                    self.get_category(index, node)
                    self.set_filtergraph()

                    self.first = False
                    self.last_time = self.begin
                    break
                elif self.last_time < self.begin:
                    if index + 1 == len(self.clip_nodes["program"]):
                        self.last = True
                    else:
                        self.last = False

                    if _playlist.start:
                        check_sync(self.begin, self._encoder)

                    self.src = node["source"]
                    self.probe.load(self.src)

                    self.url_or_live_source()
                    self.get_input()
                    self.is_source_dummy()
                    self.get_category(index, node)
                    self.set_filtergraph()

                    if not self.next_playlist:
                        # normal behavior
                        self.last_time = self.begin
                    else:
                        # when there is no time left and we are in time,
                        # set right values for new playlist
                        self.list_date = get_date(False)
                        self.last_time = _playlist.start - 5
                        self.last_mod_time = 0.0

                    break

                self.begin += self.out - self.seek
            else:
                if not _playlist.start or 'length' not in self.clip_nodes:
                    # when we reach currect end, stop script
                    messenger.info('Playlist reach End!')
                    return

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
        '-bufsize', '{}k'.format(_pre_comp.v_bufsize),
        '-c:a', 's302m', '-strict', '-2', '-ar', '48000', '-ac', '2',
        '-f', 'mpegts', '-']

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
            encoder = Popen([
                'ffplay', '-hide_banner', '-nostats', '-i', 'pipe:0'
                ] + overlay, stderr=None, stdin=PIPE, stdout=None)
        else:
            encoder = Popen([
                'ffmpeg', '-v', 'info', '-hide_banner', '-nostats',
                '-re', '-thread_queue_size', '256',
                '-i', 'pipe:0'] + overlay + _playout.post_comp_video
                + _playout.post_comp_audio + [
                    '-metadata', 'service_name=' + _playout.name,
                    '-metadata', 'service_provider=' + _playout.provider,
                    '-metadata', 'year={}'.format(year)
                ] + _playout.post_comp_extra + [_playout.out_addr], stdin=PIPE)

        if _playlist.mode and not stdin_args.folder:
            watcher = None
            get_source = GetSourceIter(encoder)
        else:
            messenger.info("start folder mode")
            media = MediaStore()
            watcher = MediaWatcher(media)
            get_source = GetSource(media)

        try:
            for src_cmd in get_source.next():
                messenger.debug('src_cmd: "{}"'.format(src_cmd))
                if src_cmd[0] == '-i':
                    current_file = src_cmd[1]
                else:
                    current_file = src_cmd[3]

                messenger.info('play: "{}"'.format(current_file))

                with Popen([
                    'ffmpeg', '-v', 'error', '-hide_banner', '-nostats'
                    ] + src_cmd + ff_pre_settings,
                        stdout=PIPE) as decoder:

                    while True:
                        buf = decoder.stdout.read(65424)
                        if not buf and decoder.poll() is not None:
                            break
                        encoder.stdin.write(buf)

        except BrokenPipeError:
            messenger.error('Broken Pipe!')
            terminate_processes(decoder, encoder, watcher)

        except SystemExit:
            messenger.info("got close command")
            terminate_processes(decoder, encoder, watcher)

        except KeyboardInterrupt:
            messenger.warning('program terminated')
            terminate_processes(decoder, encoder, watcher)

        # close encoder when nothing is to do anymore
        if encoder.poll() is None:
            encoder.terminate()

    finally:
        encoder.wait()


if __name__ == '__main__':
    if not _playlist.mode or stdin_args.folder:
        from watchdog.events import PatternMatchingEventHandler
        from watchdog.observers import Observer

    main()
