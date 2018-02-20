#!/usr/bin/python3

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
import logging
import re
import smtplib
import sys
from argparse import ArgumentParser
from ast import literal_eval
from datetime import datetime, date, timedelta
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from logging.handlers import TimedRotatingFileHandler
from os import path
from shutil import copyfileobj
from subprocess import Popen, PIPE
from threading import Thread
from time import sleep
from types import SimpleNamespace
import xml.etree.ElementTree as ET


# ------------------------------------------------------------------------------
# read variables from config file
# ------------------------------------------------------------------------------

# read config
cfg = configparser.ConfigParser()
cfg.read("/etc/ffplayout/ffplayout.conf")


_mail = SimpleNamespace(
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
    aspect=cfg.getfloat('PRE_COMPRESS', 'width') /
    cfg.getfloat('PRE_COMPRESS', 'height'),
    fps=cfg.getint('PRE_COMPRESS', 'fps'),
    v_bitrate=cfg.getint('PRE_COMPRESS', 'v_bitrate'),
    v_bufsize=cfg.getint('PRE_COMPRESS', 'v_bitrate'),
    a_bitrate=cfg.getint('PRE_COMPRESS', 'a_bitrate'),
    a_sample=cfg.getint('PRE_COMPRESS', 'a_sample')
)

_playlist = SimpleNamespace(
    path=cfg.get('PLAYLIST', 'playlist_path'),
    start=cfg.getint('PLAYLIST', 'day_start'),
    filler=cfg.get('PLAYLIST', 'filler_clip')
)

_buffer = SimpleNamespace(
    length=cfg.getint('BUFFER', 'buffer_length'),
    cli=cfg.get('BUFFER', 'buffer_cli'),
    cmd=literal_eval(cfg.get('BUFFER', 'buffer_cmd'))
)

_playout = SimpleNamespace(
    name=cfg.get('OUT', 'service_name'),
    provider=cfg.get('OUT', 'service_provider'),
    out_addr=cfg.get('OUT', 'out_addr'),
    post_comp_video=literal_eval(cfg.get('OUT', 'post_comp_video')),
    post_comp_audio=literal_eval(cfg.get('OUT', 'post_comp_audio')),
    post_comp_extra=literal_eval(cfg.get('OUT', 'post_comp_extra'))
)

# set logo filtergraph
if path.exists(cfg.get('OUT', 'logo')):
    _playout.logo = ['-thread_queue_size', '16', '-i', cfg.get('OUT', 'logo')]
    _playout.filter = [
        '-filter_complex', '[0:v][1:v]' + cfg.get('OUT', 'logo_o') + '[o]',
        '-map', '[o]', '-map', '0:a'
    ]
else:
    _playout.logo = []
    _playout.filter = []


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
class ffplayout_logger(object):
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
sys.stdout = ffplayout_logger(logger, logging.INFO)
# Replace stderr with logging to file at ERROR level
sys.stderr = ffplayout_logger(logger, logging.ERROR)


# ------------------------------------------------------------------------------
# global helper functions
# ------------------------------------------------------------------------------

# get time
def get_time(time_format):
    t = datetime.today()
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
    if get_time('hour') < _playlist.start and seek_day:
        yesterday = date.today() - timedelta(1)
        return yesterday.strftime('%Y-%m-%d')
    else:
        return datetime.now().strftime('%Y-%m-%d')


# send error messages to email addresses
def mail_or_log(message, time, path):
    if _mail.recip:
        msg = MIMEMultipart()
        msg['From'] = _mail.s_addr
        msg['To'] = _mail.recip
        msg['Subject'] = "Playout Error"
        msg.attach(MIMEText('{} {}\n{}'.format(time, message, path), 'plain'))
        text = msg.as_string()

        server = smtplib.SMTP(_mail.server, int(_mail.port))
        server.starttls()
        server.login(_mail.s_addr, _mail.s_pass)
        server.sendmail(_mail.s_addr, _mail.recip, text)
        server.quit()
    else:
        logger.error('{} {}'.format(message, path))


# calculating the size for the buffer in KB
def calc_buffer_size():
    # TODO: this calculation is only important when we compress in rawvideo
    """
    v_size = _pre_comp.w * _pre_comp.h * 3 / 2 * _pre_comp.fps * _buffer.length
    a_size = (_pre_comp.a_sample * 16 * 2 * _buffer.length) / 8
    return (v_size + a_size) / 1024
    """
    return (_pre_comp.v_bitrate + _pre_comp.a_bitrate) * 0.125 * _buffer.length


# check if processes a well
def check_process(watch_proc, terminate_proc):
    while True:
        sleep(4)
        if watch_proc.poll() is not None:
            terminate_proc.terminate()
            break


# check if path exist,
# when not send email and generate blackclip
def check_file_exist(in_file):
    if path.exists(in_file):
        return True
    else:
        return False


# seek in clip and cut the end
def seek_in_cut_end(in_file, duration, seek, out):
    if seek > 0.0:
        inpoint = ['-ss', str(seek)]
        fade_in_vid = 'fade=in:st=0:d=0.5'
        fade_in_aud = 'afade=in:st=0:d=0.5'
    else:
        inpoint = []
        fade_in_vid = 'null'
        fade_in_aud = 'anull'

    if out < duration:
        fade_out_time = out - seek - 1.0
        cut_end = ['-t', str(out - seek)]
        fade_out_vid = 'fade=out:st=' + str(fade_out_time) + ':d=1.0'
        fade_out_aud = 'afade=out:st=' + str(fade_out_time) + ':d=1.0'
    else:
        cut_end = []
        fade_out_vid = 'null'
        fade_out_aud = 'anull'

    return inpoint + ['-i', in_file] + cut_end + [
        '-vf', fade_in_vid + ',' + fade_out_vid,
        '-af', fade_in_aud + ',' + fade_out_aud
    ]


# generate a dummy clip, with black color and empty audiotrack
def gen_dummy(duration):
    return [
        '-f', 'lavfi', '-i',
        'color=s={}x{}:d={}'.format(
            _pre_comp.w, _pre_comp.h, duration
        ),
        '-f', 'lavfi', '-i', 'anullsrc=r=' + str(_pre_comp.a_sample),
        '-shortest'
    ]


# when source path exist, generate input with seek and out time
# when path not exist, get dummy clip
def src_or_dummy(src, duration, seek, out):
    if check_file_exist(src):
        if seek > 0.0 or out < duration:
            return seek_in_cut_end(src, duration, seek, out)
        else:
            return ['-i', src]
    else:
        return gen_dummy(out - seek)


# compare clip play time with real time,
# to see if we are sync
def check_sync(begin):
    time_now = get_time('full_sec')
    start = float(_playlist.start * 3600)

    t_dist = begin - time_now
    if 0 <= time_now < start and not begin == start:
        t_dist -= 86400.0

    # check that we are in tolerance time
    if not _buffer.length - 8 < t_dist < _buffer.length + 8:
        mail_or_log(
            'Playlist is not sync!', get_time(None),
            str(t_dist) + ' seconds async.'
        )


# prepare input clip
# check begin and length from clip
# return clip only if we are in 24 hours time range
def gen_input(src, begin, duration, seek, out, last):
    test_time = 86400.0
    start = float(_playlist.start * 3600)

    if begin + out - seek > test_time:
        begin -= start

    playlist_length = begin + out - seek

    if playlist_length <= test_time and not last:
        # when we are in the 24 houre range, get the clip
        return src_or_dummy(src, duration, seek, out)
    elif playlist_length < test_time and last:
        # when last clip is passed and we still have too much time left
        diff = test_time - playlist_length

        if duration < diff:
            mail_or_log(
                'Last clip is not long enough', get_time(None),
                str(diff - duration) + ' seconds are missing.'
            )
            src_cmd = src_or_dummy(src, duration, 0, duration)
        else:
            if src == _playlist.filler:
                src_cmd = src_or_dummy(src, duration, duration - diff, out)
            else:
                src_cmd = src_or_dummy(src, duration, 0, duration - diff)
    else:
        # when we over the 24 hours range,
        # calculate time and try to correct them
        diff = playlist_length - test_time
        new_len = out - seek - diff
        if new_len > 6.0:
            if src == _playlist.filler:
                # when filler is something like a clock,
                # is better to start the clip later, to play until end
                src_cmd = src_or_dummy(src, duration, seek + diff, out)
            else:
                src_cmd = src_or_dummy(src, duration, seek, out - diff)
        elif new_len > 1.0:
            src_cmd = gen_dummy(new_len)
        else:
            src_cmd = None

        return src_cmd


# test if value is float
def is_float(value, text, convert):
    try:
        float(value)
        if convert:
            return float(value)
        else:
            return ''
    except ValueError:
        return text


# validate xml values in new Thread
# and test if file path exist
def validate_thread(clip_nodes):
    def check_xml(xml_nodes):
        error = ''

        for xml_node in xml_nodes:
            if check_file_exist(xml_node.get('src')):
                a = ''
            else:
                a = 'File not exist! '

            b = is_float(xml_node.get('begin'), 'No Start Time! ', False)
            c = is_float(xml_node.get('dur'), 'No Duration! ', False)
            d = is_float(xml_node.get('in'), 'No In Value! ', False)
            e = is_float(xml_node.get('out'), 'No Out Value! ', False)

            line = a + b + c + d + e
            if line:
                error += line + 'In line: ' + str(xml_node.attrib) + '\n'

        if error:
            mail_or_log(
                'Validation error, check xml playlist, values are missing:\n',
                get_time(None), error
            )

    validate = Thread(name='check_xml', target=check_xml, args=(clip_nodes,))
    validate.daemon = True
    validate.start()


def exeption(last_time, message, path):
    src_cmd = gen_dummy(300)
    # there is still material in the buffer,
    # so we have to calculate the right seek time for the new playlist
    if last_time > 86400:
        last_time -= 86400

    time_diff = last_time - get_time('full_sec')
    last_time = get_time('full_sec')
    mail_or_log(message, get_time(None), path)

    return src_cmd, last_time, time_diff


# ------------------------------------------------------------------------------
# main functions
# ------------------------------------------------------------------------------

# read values from xml playlist
def iter_src_commands():
    last_time = get_time('full_sec')
    if 0 <= last_time < _playlist.start * 3600:
        last_time += 86400
    last_mod_time = 0.0
    time_diff = 0.0
    first = True
    list_date = get_date(True)

    while True:
        year, month, day = re.split('-', list_date)
        xml_path = path.join(_playlist.path, year, month, list_date + '.xml')

        if check_file_exist(xml_path):
            # check last modification from playlist
            mod_time = path.getmtime(xml_path)
            if mod_time > last_mod_time:
                xml_root = ET.parse(open(xml_path, "r")).getroot()
                clip_nodes = xml_root.findall('body/video')
                last_mod_time = mod_time
                logger.info('open: ' + xml_path)
                validate_thread(clip_nodes)
                last_node = clip_nodes[-1]

            # all clips in playlist except last one
            for clip_node in clip_nodes:
                src = clip_node.get('src')
                begin = is_float(clip_node.get('begin'), last_time, True)
                duration = is_float(clip_node.get('dur'), 300, True)
                seek = is_float(clip_node.get('in'), 0, True)
                out = is_float(clip_node.get('out'), 300, True)

                # first time we end up here
                if first and last_time + time_diff < begin + duration:
                    # calculate seek time
                    seek = last_time - begin + seek + time_diff
                    src_cmd = gen_input(
                        src, begin, duration, seek, out, False
                    )

                    time_diff = 0.0
                    first = False
                    last_time = begin
                    break
                elif last_time < begin:
                    if clip_node == last_node:
                        # after last item is loaded,
                        # set right values for new playlist
                        list_date = get_date(False)
                        last_time = float(_playlist.start * 3600 - 5)
                        last_mod_time = 0.0
                        last = True
                    else:
                        last_time = begin
                        last = False

                    check_sync(begin)

                    src_cmd = gen_input(
                        src, begin, duration, seek, out, last
                    )

                    break
            else:
                # when playlist exist but is empty, or not long enough,
                # generate dummy and send log
                src_cmd, last_time, time_diff = exeption(
                    last_time, 'Playlist is not valid!', xml_path
                )

                first = True
                last_mod_time = 0.0

        else:
            # when we have no playlist for the current day,
            # then we generate a black clip
            # and calculate the seek in time, for when the playlist comes back
            src_cmd, last_time, time_diff = exeption(
                last_time, 'Playlist not exist:', xml_path
            )

            first = True
            last_mod_time = 0.0

        if src_cmd is not None:
            yield src_cmd, last_time


# independent thread for clip preparation
def play_clips(out_file, iter_src_commands):
    # infinit loop
    # send current file from xml playlist to stdin from buffer
    for src_cmd, last_time in iter_src_commands:
        if last_time > 86400:
            tm_str = str(timedelta(seconds=int(last_time - 86400)))
        else:
            tm_str = str(timedelta(seconds=int(last_time)))

        logger.info('play at "{}":  {}'.format(tm_str, src_cmd))

        try:
            filePiper = Popen(
                [
                    'ffmpeg', '-v', 'error', '-hide_banner', '-nostats'
                ] + src_cmd +
                [
                    '-s', '{}x{}'.format(_pre_comp.w, _pre_comp.h),
                    '-aspect', str(_pre_comp.aspect),
                    '-pix_fmt', 'yuv420p', '-r', str(_pre_comp.fps),
                    '-af', 'apad', '-shortest',
                    '-c:v', 'mpeg2video', '-intra',
                    '-b:v', '{}k'.format(_pre_comp.v_bitrate),
                    '-minrate', '{}k'.format(_pre_comp.v_bitrate),
                    '-maxrate', '{}k'.format(_pre_comp.v_bitrate),
                    '-bufsize', '{}k'.format(_pre_comp.v_bufsize),
                    '-c:a', 'mp2', '-b:a', '{}k'.format(_pre_comp.a_bitrate),
                    '-ar', str(_pre_comp.a_sample), '-ac', '2',
                    '-threads', '2', '-f', 'mpegts', '-'
                ],
                stdout=PIPE,
                bufsize=0
            )

            copyfileobj(filePiper.stdout, out_file)
        finally:
            filePiper.wait()


def main():
    year, month, _day = re.split('-', get_date(False))
    try:
        # open a buffer for the streaming pipeline
        # stdin get the files loop
        # stdout pipes to ffmpeg rtmp streaming
        mbuffer = Popen(
            [_buffer.cli] + list(_buffer.cmd) +
            [str(calc_buffer_size()) + 'k'],
            stdin=PIPE,
            stdout=PIPE,
            bufsize=0
        )
        try:
            # playout to rtmp
            playout = Popen(
                [
                    'ffmpeg', '-v', 'info', '-hide_banner', '-nostats', '-re',
                    '-thread_queue_size', '256', '-fflags', '+igndts',
                    '-i', 'pipe:0', '-fflags', '+genpts'
                ] +
                list(_playout.logo) +
                list(_playout.filter) +
                list(_playout.post_comp_video) +
                list(_playout.post_comp_audio) +
                [
                    '-metadata', 'service_name=' + _playout.name,
                    '-metadata', 'service_provider=' + _playout.provider,
                    '-metadata', 'year=' + year
                ] +
                list(_playout.post_comp_extra) +
                [
                    _playout.out_addr
                ],
                stdin=mbuffer.stdout,
                bufsize=0
            )

            play_thread = Thread(
                name='play_clips', target=play_clips, args=(
                    mbuffer.stdin,
                    iter_src_commands(),
                )
            )
            play_thread.daemon = True
            play_thread.start()

            check_process(playout, mbuffer)
        finally:
            playout.wait()
    finally:
        mbuffer.wait()


if __name__ == '__main__':
    main()
