#!/usr/bin/env python3

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
import json
import logging
import os
import smtplib
import socket
import sys
from argparse import ArgumentParser
from ast import literal_eval
from datetime import date, datetime, timedelta
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from email.utils import formatdate
from logging.handlers import TimedRotatingFileHandler
from shutil import copyfileobj
from subprocess import PIPE, CalledProcessError, Popen, check_output
from threading import Thread
from time import sleep
from types import SimpleNamespace

# ------------------------------------------------------------------------------
# read variables from config file
# ------------------------------------------------------------------------------

# read config
cfg = configparser.ConfigParser()
if os.path.exists("/etc/ffplayout/ffplayout.conf"):
    cfg.read("/etc/ffplayout/ffplayout.conf")
else:
    cfg.read("ffplayout.conf")

_general = SimpleNamespace(
    stop=cfg.getboolean('GENERAL', 'stop_on_error'),
    threshold=cfg.getfloat('GENERAL', 'stop_threshold')
)

_mail = SimpleNamespace(
    subject=cfg.get('MAIL', 'subject'),
    server=cfg.get('MAIL', 'smpt_server'),
    port=cfg.getint('MAIL', 'smpt_port'),
    s_addr=cfg.get('MAIL', 'sender_addr'),
    s_pass=cfg.get('MAIL', 'sender_pass'),
    recip=cfg.get('MAIL', 'recipient')
)

_log = SimpleNamespace(
    path=cfg.get('LOGGING', 'log_file'),
    level=cfg.get('LOGGING', 'log_level')
)

_pre_comp = SimpleNamespace(
    w=cfg.getint('PRE_COMPRESS', 'width'),
    h=cfg.getint('PRE_COMPRESS', 'height'),
    aspect=cfg.getfloat(
        'PRE_COMPRESS', 'width') / cfg.getfloat('PRE_COMPRESS', 'height'),
    fps=cfg.getint('PRE_COMPRESS', 'fps'),
    v_bitrate=cfg.getint('PRE_COMPRESS', 'v_bitrate'),
    v_bufsize=cfg.getint('PRE_COMPRESS', 'v_bitrate') / 2,
    logo=cfg.get('PRE_COMPRESS', 'logo'),
    logo_filter=cfg.get('PRE_COMPRESS', 'logo_filter'),
    protocols=cfg.get('PRE_COMPRESS', 'live_protocols'),
    copy=cfg.getboolean('PRE_COMPRESS', 'copy_mode'),
    copy_settings=literal_eval(cfg.get('PRE_COMPRESS', 'ffmpeg_copy_settings'))
)

_playlist = SimpleNamespace(
    path=cfg.get('PLAYLIST', 'playlist_path'),
    start=cfg.getint('PLAYLIST', 'day_start'),
    filler=cfg.get('PLAYLIST', 'filler_clip'),
    blackclip=cfg.get('PLAYLIST', 'blackclip'),
    shift=cfg.getint('PLAYLIST', 'time_shift'),
    map_ext=cfg.get('PLAYLIST', 'map_extension')
)

_buffer = SimpleNamespace(
    length=cfg.getint('BUFFER', 'buffer_length'),
    tol=cfg.getfloat('BUFFER', 'buffer_tolerance'),
    cli=cfg.get('BUFFER', 'buffer_cli'),
    cmd=literal_eval(cfg.get('BUFFER', 'buffer_cmd'))
)

_playout = SimpleNamespace(
    preview=cfg.getboolean('OUT', 'preview'),
    name=cfg.get('OUT', 'service_name'),
    provider=cfg.get('OUT', 'service_provider'),
    out_addr=cfg.get('OUT', 'out_addr'),
    post_comp_video=literal_eval(cfg.get('OUT', 'post_comp_video')),
    post_comp_audio=literal_eval(cfg.get('OUT', 'post_comp_audio')),
    post_comp_extra=literal_eval(cfg.get('OUT', 'post_comp_extra')),
    post_comp_copy=literal_eval(cfg.get('OUT', 'post_comp_copy'))
)


# ------------------------------------------------------------------------------
# logging
# ------------------------------------------------------------------------------

stdin_parser = ArgumentParser(description="python and ffmpeg based playout")
stdin_parser.add_argument(
    "-l", "--log", help="file to write log to (default '" + _log.path + "')"
)

# If the log file is specified on the command line then override the default
stdin_args = stdin_parser.parse_args()
if stdin_args.log:
        _log.path = stdin_args.log

logger = logging.getLogger(__name__)
logger.setLevel(_log.level)
handler = TimedRotatingFileHandler(_log.path, when="midnight", backupCount=5)
formatter = logging.Formatter('[%(asctime)s] [%(levelname)s]  %(message)s')
handler.setFormatter(formatter)
logger.addHandler(handler)


# capture stdout and sterr in the log
class PlayoutLogger(object):
    def __init__(self, logger, level):
        self.logger = logger
        self.level = level

    def write(self, message):
        # Only log if there is a message (not just a new line)
        if message.rstrip() != "":
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

# send error messages to email addresses
def mailer(message, time, path):
    if _mail.recip:
        msg = MIMEMultipart()
        msg['From'] = _mail.s_addr
        msg['To'] = _mail.recip
        msg['Subject'] = _mail.subject
        msg["Date"] = formatdate(localtime=True)
        msg.attach(MIMEText('{} {}\n{}'.format(time, message, path), 'plain'))
        text = msg.as_string()

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


# ------------------------------------------------------------------------------
# global helper functions
# ------------------------------------------------------------------------------

# get time
def get_time(time_format):
    t = datetime.today() + timedelta(seconds=_playlist.shift)
    if time_format == 'hour':
        return t.hour
    elif time_format == 'full_sec':
        sec = float(t.hour * 3600 + t.minute * 60 + t.second)
        micro = float(t.microsecond) / 1000000
        return sec + micro
    else:
        return t.strftime("%H:%M:%S")


# get date
def get_date(seek_day):
    d = date.today() + timedelta(seconds=_playlist.shift)
    if get_time('hour') < _playlist.start and seek_day:
        yesterday = d - timedelta(1)
        return yesterday.strftime('%Y-%m-%d')
    else:
        return d.strftime('%Y-%m-%d')


# check if input file exist
def file_exist(in_file):
    if os.path.exists(in_file):
        return True
    else:
        return False


# test if value is float
def is_float(value):
    try:
        float(value)
        return True
    except ValueError:
        return False


# test if value is int
def is_int(value):
    try:
        int(value)
        return True
    except ValueError:
        return False


# calculating the size for the buffer in KB
def calc_buffer_size():
    # in copy mode files has normally smaller bit rate,
    # so we calculate the size different
    if _pre_comp.copy:
        list_date = get_date(True)
        year, month, day = list_date.split('-')
        json_file = os.path.join(
            _playlist.path, year, month, list_date + '.json')

        if file_exist(json_file):
            with open(json_file) as f:
                clip_nodes = json.load(f)

            if _playlist.map_ext:
                _ext = literal_eval(_playlist.map_ext)
                source = clip_nodes["program"][0]["source"].replace(
                    _ext[0], _ext[1])
            else:
                source = clip_nodes["program"][0]["source"]

            cmd = [
                'ffprobe', '-v', 'error', '-show_entries', 'format=bit_rate',
                '-of', 'default=noprint_wrappers=1:nokey=1', source]
            bite_rate = check_output(cmd).decode('utf-8')

            if is_int(bite_rate):
                bite_rate = int(bite_rate) / 1024
            else:
                bite_rate = 4000

            return int(bite_rate * 0.125 * _buffer.length)
        else:
            return 5000
    else:
        return int((_pre_comp.v_bitrate * 0.125 + 281.25) * _buffer.length)


# check if processes a well
def check_process(play_thread, playout, mbuffer):
    while True:
        sleep(4)
        if playout.poll() is not None:
            logger.error(
                'postprocess is not alive anymore, terminate ffplayout!')
            mbuffer.terminate()
            break

        if not play_thread.is_alive():
            logger.error(
                'preprocess is not alive anymore, terminate ffplayout!')
            mbuffer.terminate()
            break


# compare clip play time with real time,
# to see if we are sync
def check_sync(begin):
    time_now = get_time('full_sec')
    start = float(_playlist.start * 3600)

    # in copy mode buffer length can not be calculatet correctly...
    if _pre_comp.copy:
        tolerance = 60
    else:
        tolerance = _buffer.tol * 4

    t_dist = begin - time_now
    if 0 <= time_now < start and not begin == start:
        t_dist -= 86400.0

    # check that we are in tolerance time
    if not _buffer.length - tolerance < t_dist < _buffer.length + tolerance:
        mailer(
            'Playlist is not sync!', get_time(None),
            '{} seconds async'.format(t_dist)
        )
        logger.error('Playlist is {} seconds async!'.format(t_dist))

    if _general.stop and abs(t_dist - _buffer.length) > _general.threshold:
        logger.error('Sync tolerance value exceeded, program is terminated')
        sys.exit(1)


# check last item, when it is None or a dummy clip,
# set true and seek in playlist
def check_last_item(src_cmd, last_time, last):
    if src_cmd is None and not last:
        first = True
        last_time = get_time('full_sec')
        if 0 <= last_time < _playlist.start * 3600:
            last_time += 86400

    elif 'lavfi' in src_cmd and not last:
        first = True
        last_time = get_time('full_sec') + _buffer.length + _buffer.tol
        if 0 <= last_time < _playlist.start * 3600:
            last_time += 86400
    else:
        first = False

    return first, last_time


# check begin and length
def check_start_and_length(json_nodes, counter):
    # check start time and set begin
    if "begin" in json_nodes:
        h, m, s = json_nodes["begin"].split(':')
        if is_float(h) and is_float(m) and is_float(s):
            begin = float(h) * 3600 + float(m) * 60 + float(s)
        else:
            begin = -100.0
    else:
        begin = -100.0

    # check if playlist is long enough
    if "length" in json_nodes:
        l_h, l_m, l_s = json_nodes["length"].split(':')
        if is_float(l_h) and is_float(l_m) and is_float(l_s):
            length = float(l_h) * 3600 + float(l_m) * 60 + float(l_s)

            start = float(_playlist.start * 3600)
            total_play_time = begin + counter - start

            if "date" in json_nodes:
                date = json_nodes["date"]
            else:
                date = get_date(True)

            if total_play_time < length - 5:
                mailer(
                    'json playlist ({}) is not long enough!'.format(date),
                    get_time(None), "total play time is: {}".format(
                        timedelta(seconds=total_play_time))
                )
                logger.error('Playlist is only {} hours long!'.format(
                    timedelta(seconds=total_play_time)))


# validate json values in new Thread
# and test if file path exist
# TODO: we need better and unique validation,
# now it is messy - the file get readed twice
# and values get multiple time evaluate
# IDEA: open one time the playlist,
# not in a thread and build from it a new clean dictionary
def validate_thread(clip_nodes):
    def check_json(json_nodes):
        error = ''
        counter = 0

        # check if all values are valid
        for node in json_nodes["program"]:
            if _playlist.map_ext:
                _ext = literal_eval(_playlist.map_ext)
                source = node["source"].replace(
                    _ext[0], _ext[1])
            else:
                source = node["source"]

            prefix = source.split('://')[0]

            if prefix in _pre_comp.protocols:
                cmd = [
                    'ffprobe', '-v', 'error',
                    '-show_entries', 'format=duration',
                    '-of', 'default=noprint_wrappers=1:nokey=1', source]

                try:
                    output = check_output(cmd).decode('utf-8')
                except CalledProcessError:
                    output = '404'

                if '404' in output:
                    a = 'Stream not exist: {}\n'.format(source)
                else:
                    a = ''
            elif file_exist(source):
                a = ''
            else:
                a = 'File not exist: {}\n'.format(source)

            if is_float(node["in"]) and is_float(node["out"]):
                b = ''
                counter += node["out"] - node["in"]
            else:
                b = 'Missing Value in: {}\n'.format(node)

            c = '' if is_float(node["duration"]) else 'No duration Value! '

            line = a + b + c
            if line:
                logger.error('Validation error in line: {}'.format(line))
                error += line + 'In line: {}\n'.format(node)

        if error:
            mailer(
                'Validation error, check json playlist, values are missing:\n',
                get_time(None), error
            )

        check_start_and_length(json_nodes, counter)

    validate = Thread(name='check_json', target=check_json, args=(clip_nodes,))
    validate.daemon = True
    validate.start()


# seek in clip
def seek_in(seek):
    if seek > 0.0:
        return ['-ss', str(seek)]
    else:
        return []


# cut clip length
def cut_end(duration, seek, out):
    if out < duration:
        return ['-t', str(out - seek)]
    else:
        return []


# generate a dummy clip, with black color and empty audiotrack
def gen_dummy(duration):
    if _pre_comp.copy:
        return ['-i', _playlist.blackclip]
    else:
        return [
            '-f', 'lavfi', '-i',
            'color=s={}x{}:d={}:r={}'.format(
                _pre_comp.w, _pre_comp.h, duration, _pre_comp.fps
            ),
            '-f', 'lavfi', '-i', 'anullsrc=r=48000',
            '-shortest'
        ]


# when source path exist, generate input with seek and out time
# when path not exist, generate dummy clip
def src_or_dummy(src, duration, seek, out, dummy_len=None):
    if src:
        prefix = src.split('://')[0]

        # check if input is a live source
        if prefix in _pre_comp.protocols:
            return seek_in(seek) + ['-i', src] + cut_end(duration, seek, out)
        elif file_exist(src):
            return seek_in(seek) + ['-i', src] + cut_end(duration, seek, out)
        else:
            mailer('Clip not exist:', get_time(None), src)
            logger.error('Clip not exist: {}'.format(src))
            if dummy_len and not _pre_comp.copy:
                return gen_dummy(dummy_len)
            else:
                return gen_dummy(out - seek)
    else:
        return gen_dummy(dummy_len)


# prepare input clip
# check begin and length from clip
# return clip only if we are in 24 hours time range
def gen_input(src, begin, dur, seek, out, last):
    start = float(_playlist.start * 3600)
    day_in_sec = 86400.0
    ref_time = day_in_sec + start
    time = get_time('full_sec')

    if 0 <= time < start:
        time += day_in_sec

    # calculate time difference to see if we are sync
    time_diff = _buffer.length + _buffer.tol + out - seek + time

    if (time_diff <= ref_time or begin < day_in_sec) and not last:
        # when we are in the 24 houre range, get the clip
        return src_or_dummy(src, dur, seek, out, 20), None
    elif time_diff < ref_time and last:
        # when last clip is passed and we still have too much time left
        # check if duration is larger then out - seek
        time_diff = _buffer.length + _buffer.tol + dur + time
        new_len = dur - (time_diff - ref_time)
        logger.info('we are under time, new_len is: {}'.format(new_len))

        if time_diff >= ref_time:
            if src == _playlist.filler:
                # when filler is something like a clock,
                # is better to start the clip later and to play until end
                src_cmd = src_or_dummy(src, dur, dur - new_len, dur)
            else:
                src_cmd = src_or_dummy(src, dur, 0, new_len)
        else:
            src_cmd = src_or_dummy(src, dur, 0, dur)

            mailer(
                'Playlist is not long enough:', get_time(None),
                '{} seconds needed.'.format(new_len)
            )
            logger.error('Playlist is {} seconds to short'.format(new_len))

        return src_cmd, new_len - dur

    elif time_diff > ref_time:
        new_len = out - seek - (time_diff - ref_time)
        # when we over the 24 hours range, trim clip
        logger.info('we are over time, new_len is: {}'.format(new_len))

        if new_len > 5.0:
            if src == _playlist.filler:
                src_cmd = src_or_dummy(src, dur, out - new_len, out)
            else:
                src_cmd = src_or_dummy(src, dur, seek, new_len)
        elif new_len > 1.0:
            src_cmd = gen_dummy(new_len)
        else:
            src_cmd = None

        return src_cmd, 0.0


# blend logo and fade in / fade out
def build_filtergraph(first, duration, seek, out, ad, ad_last, ad_next, dummy):
    length = out - seek - 1.0
    logo_chain = []
    logo_filter = []
    video_chain = []
    audio_chain = []
    video_map = ['-map', '[logo]']

    scale = 'scale={}:{},setdar=dar={}[s]'.format(
        _pre_comp.w, _pre_comp.h, _pre_comp.aspect)

    if seek > 0.0 and not first:
        video_chain.append('fade=in:st=0:d=0.5')
        audio_chain.append('afade=in:st=0:d=0.5')

    if out < duration:
        video_chain.append('fade=out:st={}:d=1.0'.format(length))
        audio_chain.append('apad,afade=out:st={}:d=1.0'.format(length))
    else:
        audio_chain.append('apad')

    if video_chain:
        video_fade = '[s]{}[v]'.format(','.join(video_chain))
    else:
        video_fade = '[s]null[v]'

    audio_filter = [
        '-filter_complex', '[0:a]{}[a]'.format(','.join(audio_chain))]

    audio_map = ['-shortest', '-map', '[a]']

    if os.path.exists(_pre_comp.logo):
        if not ad:
            opacity = 'format=rgba,colorchannelmixer=aa=0.7'
            loop = 'loop=loop={}:size=1:start=0'.format(
                    (out - seek) * _pre_comp.fps)
            logo_chain.append('movie={},{},{}'.format(
                    _pre_comp.logo, loop, opacity))
        if ad_last:
            logo_chain.append('fade=in:st=0:d=1.0:alpha=1')
        if ad_next:
            logo_chain.append('fade=out:st={}:d=1.0:alpha=1'.format(length))

        if not ad:
            logo_filter = '{}[l];[v][l]{}[logo]'.format(
                    ','.join(logo_chain), _pre_comp.logo_filter)
        else:
            logo_filter = '[v]null[logo]'
    else:
        logo_filter = '[v]null[logo]'

    video_filter = [
        '-filter_complex', '[0:v]{};{};{}'.format(
            scale, video_fade, logo_filter)]

    if _pre_comp.copy:
        return []
    elif dummy:
        return video_filter + video_map
    else:
        return video_filter + audio_filter + video_map + audio_map


# ------------------------------------------------------------------------------
# main functions
# ------------------------------------------------------------------------------

# read values from json playlist
class GetSourceIter:
    def __init__(self):
        self.last_time = 0.0
        self.last_mod_time = 0.0
        self.json_file = None
        self.clip_nodes = None
        self.src_cmd = None
        self.filtergraph = []
        self.first = True
        self.last = False
        self.list_date = get_date(True)
        self.is_dummy = False
        self.dummy_len = 60
        self.has_begin = False
        self.init_time = get_time('full_sec')

        self.src = None
        self.seek = 0
        self.out = 60
        self.duration = 60
        self.ad = False
        self.ad_last = False
        self.ad_next = False

    def get_playlist(self):
        year, month, day = self.list_date.split('-')
        self.json_file = os.path.join(
         _playlist.path, year, month, self.list_date + '.json')

        if file_exist(self.json_file):
            # check last modification from playlist
            mod_time = os.path.getmtime(self.json_file)
            if mod_time > self.last_mod_time:
                with open(self.json_file, 'r') as f:
                    self.clip_nodes = json.load(f)

                self.last_mod_time = mod_time
                logger.info('open: ' + self.json_file)
                validate_thread(self.clip_nodes)
        else:
            # when we have no playlist for the current day,
            # then we generate a black clip
            # and calculate the seek in time, for when the playlist comes back
            self.error_handling('Playlist not exist:')

        # when begin is in playlist, get start time from it
        if self.clip_nodes and "begin" in self.clip_nodes:
            h, m, s = self.clip_nodes["begin"].split(':')
            if is_float(h) and is_float(m) and is_float(s):
                self.has_begin = True
                self.init_time = float(h) * 3600 + float(m) * 60 + float(s)
        else:
            self.has_begin = False

    def url_or_live_source(self):
        prefix = self.src.split('://')[0]

        # check if input is a live source
        if prefix in _pre_comp.protocols:
            cmd = [
                'ffprobe', '-v', 'error', '-show_entries', 'format=duration',
                '-of', 'default=noprint_wrappers=1:nokey=1', self.src]

            try:
                output = check_output(cmd).decode('utf-8')
            except CalledProcessError:
                output = None

            if not output:
                self.duration = 60
                mailer('Clip not exist:', get_time(None), self.src)
                logger.error('Clip not exist: {}'.format(self.src))
                if self.dummy_len and not _pre_comp.copy:
                    self.src = None
                else:
                    self.src = None
                    self.dummy_len = 20
            elif is_float(output):
                self.duration = float(output)
            else:
                self.duration = 86400
                self.out = self.out - self.seek
                self.seek = 0

    def map_extension(self, node):
        if _playlist.map_ext:
            _ext = literal_eval(_playlist.map_ext)
            self.src = node["source"].replace(
                _ext[0], _ext[1])
        else:
            self.src = node["source"]

    def clip_length(self, node):
        if is_float(node["in"]):
            self.seek = node["in"]
        else:
            self.seek = 0

        if is_float(node["duration"]):
            self.duration = node["duration"]
        else:
            self.duration = self.dummy_len

        if is_float(node["out"]):
            self.out = node["out"]
        else:
            self.out = self.duration

    def get_category(self, index, node):
        if 'category' in node:
            if index - 1 >= 0:
                last_category = self.clip_nodes[
                    "program"][index - 1]["category"]
            else:
                last_category = "noad"

            if index + 2 <= len(self.clip_nodes["program"]):
                next_category = self.clip_nodes[
                    "program"][index + 1]["category"]
            else:
                next_category = "noad"

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
            self.first, self.duration, self.seek, self.out,
            self.ad, self.ad_last, self.ad_next, self.is_dummy)

    def check_source(self):
        if 'anullsrc=r=48000' in self.src_cmd:
            self.is_dummy = True
        else:
            self.is_dummy = False

    def error_handling(self, message):
        self.is_dummy = True
        self.src_cmd = gen_dummy(self.dummy_len)

        if self.last:
            self.last_time = float(_playlist.start * 3600 - 5)
            self.first = False
        else:
            self.last_time = (
                get_time('full_sec') + self.dummy_len
                + _buffer.length + _buffer.tol
            )

            if 0 <= self.last_time < _playlist.start * 3600:
                self.last_time += 86400

            self.first = True

        mailer(message, get_time(None), self.json_file)
        logger.error('{} {}'.format(message, self.json_file))

        self.begin = get_time('full_sec') + _buffer.length + _buffer.tol
        self.last = False
        self.dummy_len = 60
        self.last_mod_time = 0.0

    def next(self):
        while True:
            self.get_playlist()

            if self.clip_nodes is None:
                self.is_dummy = True
                self.set_filtergraph()
                yield self.src_cmd, self.filtergraph
                continue

            # when last clip is None or a dummy,
            # we have to jump to the right place in the playlist
            self.first, self.last_time = check_last_item(
                self.src_cmd, self.last_time, self.last)

            self.begin = self.init_time

            # loop through all clips in playlist
            for index, node in enumerate(self.clip_nodes["program"]):
                self.map_extension(node)
                self.clip_length(node)

                # first time we end up here
                if self.first and \
                        self.last_time < self.begin + self.out - self.seek:
                    if self.has_begin:
                        # calculate seek time
                        self.seek = self.last_time - self.begin + self.seek

                    self.url_or_live_source()
                    self.src_cmd, self.time_left = gen_input(
                        self.src, self.begin, self.duration,
                        self.seek, self.out, False
                    )

                    self.check_source()
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

                    if self.has_begin:
                        check_sync(self.begin)

                    self.url_or_live_source()
                    self.src_cmd, self.time_left = gen_input(
                        self.src, self.begin, self.duration,
                        self.seek, self.out, self.last
                    )

                    self.check_source()
                    self.get_category(index, node)
                    self.set_filtergraph()

                    if self.time_left is None:
                        # normal behavior
                        self.last_time = self.begin
                    elif self.time_left > 0.0:
                        # when playlist is finish and we have time left
                        self.list_date = get_date(False)
                        self.last_time = self.begin
                        self.dummy_len = self.time_left

                    else:
                        # when there is no time left and we are in time,
                        # set right values for new playlist
                        self.list_date = get_date(False)
                        self.last_time = float(_playlist.start * 3600 - 5)
                        self.last_mod_time = 0.0

                    break

                self.begin += self.out - self.seek
            else:
                # when we reach currect end, stop script
                if "begin" not in self.clip_nodes or \
                    "length" not in self.clip_nodes and \
                        self.begin < get_time('full_sec'):
                    logger.info('Playlist reach End!')
                    return

                # when playlist exist but is empty, or not long enough,
                # generate dummy and send log
                self.error_handling('Playlist is not valid!')

            if self.src_cmd is not None:
                yield self.src_cmd, self.filtergraph


# independent thread for clip preparation
def play_clips(out_file, GetSourceIter):
    # send current file to buffer stdin
    iter = GetSourceIter()

    for src_cmd, filtergraph in iter.next():
        if _pre_comp.copy:
            ff_pre_settings = _pre_comp.copy_settings
        else:
            ff_pre_settings = filtergraph + [
                '-pix_fmt', 'yuv420p', '-r', str(_pre_comp.fps),
                '-c:v', 'mpeg2video', '-intra',
                '-b:v', '{}k'.format(_pre_comp.v_bitrate),
                '-minrate', '{}k'.format(_pre_comp.v_bitrate),
                '-maxrate', '{}k'.format(_pre_comp.v_bitrate),
                '-bufsize', '{}k'.format(_pre_comp.v_bufsize),
                '-c:a', 's302m', '-strict', '-2', '-ar', '48000', '-ac', '2',
                '-threads', '2', '-f', 'mpegts', '-'
            ]

        try:
            if src_cmd[0] == '-i':
                current_file = src_cmd[1]
            else:
                current_file = src_cmd[3]

            logger.info('play: "{}"'.format(current_file))

            file_piper = Popen(
                [
                    'ffmpeg', '-v', 'error', '-hide_banner', '-nostats'
                ] + src_cmd + list(ff_pre_settings),
                stdout=PIPE,
                bufsize=0
            )

            copyfileobj(file_piper.stdout, out_file)
        finally:
            file_piper.wait()


def main():
    year = get_date(False).split('-')[0]
    try:
        # open a buffer for the streaming pipeline
        # stdin get the files loop
        # stdout pipes to ffmpeg rtmp streaming
        mbuffer = Popen(
            [_buffer.cli] + list(_buffer.cmd)
            + ['{}k'.format(calc_buffer_size())],
            stdin=PIPE,
            stdout=PIPE,
            bufsize=0
        )
        try:
            if _playout.preview:
                # preview playout to player
                playout = Popen([
                    'ffplay', '-v', 'error',
                    '-hide_banner', '-nostats', '-i', 'pipe:0'],
                    stdin=mbuffer.stdout,
                    bufsize=0
                    )
            else:
                # playout to rtmp
                if _pre_comp.copy:
                    playout_pre = [
                        'ffmpeg', '-v', 'info', '-hide_banner', '-nostats',
                        '-re', '-i', 'pipe:0', '-c', 'copy'
                    ] + _playout.post_comp_copy
                else:
                    playout_pre = [
                        'ffmpeg', '-v', 'info', '-hide_banner', '-nostats',
                        '-re', '-thread_queue_size', '256',
                        '-fflags', '+igndts', '-i', 'pipe:0',
                        '-fflags', '+genpts'
                    ] + _playout.post_comp_video + \
                        _playout.post_comp_audio

                playout = Popen(
                    list(playout_pre)
                    + [
                        '-metadata', 'service_name=' + _playout.name,
                        '-metadata', 'service_provider=' + _playout.provider,
                        '-metadata', 'year=' + year
                    ] + list(_playout.post_comp_extra)
                    + [
                        _playout.out_addr
                    ],
                    stdin=mbuffer.stdout,
                    bufsize=0
                )

            play_thread = Thread(
                name='play_clips', target=play_clips, args=(
                    mbuffer.stdin,
                    GetSourceIter,
                )
            )
            play_thread.daemon = True
            play_thread.start()

            check_process(play_thread, playout, mbuffer)
        finally:
            playout.wait()
    finally:
        mbuffer.wait()


if __name__ == '__main__':
    main()
