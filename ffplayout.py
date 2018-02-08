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
    a_sample=cfg.getint('PRE_COMPRESS', 'a_sample'),
)

_playlist = SimpleNamespace(
    path=cfg.get('PLAYLIST', 'playlist_path'),
    start=cfg.getint('PLAYLIST', 'day_start')
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
    _playout.logo = ['-thread_queue_size', '512', '-i', cfg.get('OUT', 'logo')]
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
        return t.hour * 3600 + t.minute * 60 + t.second
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
def send_mail(message, time, path):
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


# calculating the size for the buffer in bytes
def calc_buffer_size():
    return (_pre_comp.v_bitrate + _pre_comp.a_bitrate) * _buffer.length


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
        send_mail('File not exist:', get_time(None), in_file)
        return False


# seek in clip and cut the end
def seek_in_cut_end(in_file, duration, seek, out):
    if seek > 0.00:
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


# prepare input clip
def prepare_input(src, duration, seek, out):
    if check_file_exist(src):
        if seek > 0.00 or out < duration:
            src_cmd = seek_in_cut_end(src, duration, seek, out)
        else:
            src_cmd = ['-i', src]
    else:
        if seek > 0.00:
            duration = duration - seek
        if out < duration:
            duration = duration - out
        src_cmd = gen_dummy(duration)
    return src_cmd


# last clip can be a filler
# so we get the IN point and calculate the new duration
# if the new duration is smaller then 6 sec put a blank clip
def prepare_last_clip(in_node):
    src = in_node.get('src')
    duration = float(in_node.get('dur'))
    seek = float(in_node.get('in'))
    out = float(in_node.get('out'))
    tmp_dur = duration - seek

    if tmp_dur > 6.00:
        if check_file_exist(src):
            src_cmd = seek_in_cut_end(src, duration, seek, out)
        else:
            src_cmd = gen_dummy(tmp_dur)
    elif tmp_dur > 1.00:
        src_cmd = gen_dummy(tmp_dur)
    else:
        src_cmd = None

    return src_cmd


# ------------------------------------------------------------------------------
# main functions
# ------------------------------------------------------------------------------

# read values from xml playlist
def iter_src_commands():
    last_time = get_time('full_sec')
    if 0 <= last_time < _playlist.start * 3600:
        last_time += 86400
    last_mod_time = 0.00
    time_diff = 0.00
    first_in = True
    last_out = False
    time_in_buffer = 0.0

    while True:
        # switch playlist after last clip from day befor
        if last_out:
            if time_in_buffer > float(_buffer.length):
                # wait to sync time
                wait = time_in_buffer - float(_buffer.length)
                logger.info('Wait for: ' + str(wait) + ' seconds.')
                sleep(wait)
            list_date = get_date(False)
            last_out = False
        else:
            list_date = get_date(True)

        year, month, _day = re.split('-', list_date)
        xml_path = path.join(_playlist.path, year, month, list_date + '.xml')

        if check_file_exist(xml_path):
            # check last modification from playlist
            mod_time = path.getmtime(xml_path)
            if mod_time > last_mod_time:
                xml_root = ET.parse(open(xml_path, "r")).getroot()
                clip_nodes = xml_root.findall('body/video')
                last_mod_time = mod_time
                logger.info('open: ' + xml_path)

            # all clips in playlist except last one
            for clip_node in clip_nodes[:-1]:
                src = clip_node.get('src')
                begin = float(clip_node.get('begin'))
                duration = float(clip_node.get('dur'))
                seek = float(clip_node.get('in'))
                out = float(clip_node.get('out'))

                if first_in:
                    # first time we end up here
                    if last_time < begin + duration:
                        # calculate seek time
                        init_seek = last_time - begin + seek + time_diff
                        src_cmd = prepare_input(src, duration, init_seek, out)

                        time_diff = 0.00
                        first_in = False

                        last_time = begin
                        break
                else:
                    if last_time < begin:
                        src_cmd = prepare_input(src, duration, seek, out)
                        last_time = begin
                        break
            else:
                # last clip in playlist
                if last_time > 86400:
                    add_sec = 86400
                else:
                    add_sec = 0
                # calculate real time in buffer
                time_in_buffer = last_time - get_time('full_sec') - add_sec
                begin = float(_playlist.start * 3600 - 5)
                src_cmd = prepare_last_clip(clip_nodes[-1])
                last_time = begin
                list_date = get_date(True)
                last_mod_time = 0.00
                last_out = True
        else:
            # when we have no playlist for the current day,
            # then we generate a black clip
            # and calculate the seek in time, for when the playlist comes back
            src_cmd = gen_dummy(300)
            last_time += 300
            last_mod_time = 0.00

            # there is still material in the buffer,
            # so we have to calculate the right seek time for the new playlist
            # time_diff: is the real time what we have in buffer
            if last_time > 86400:
                time_val = last_time - 86400
            else:
                time_val = last_time

            time_diff = time_val - get_time('full_sec')
            first_in = True

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
                    '-c:v', 'mpeg2video', '-g', '12', '-bf', '2',
                    '-b:v', '{}k'.format(_pre_comp.v_bitrate),
                    '-minrate', '{}k'.format(_pre_comp.v_bitrate),
                    '-maxrate', '{}k'.format(_pre_comp.v_bitrate),
                    '-bufsize', '{}k'.format(_pre_comp.v_bufsize),
                    '-c:a', 'mp2', '-b:a', '{}k'.format(_pre_comp.a_bitrate),
                    '-ar', str(_pre_comp.a_sample), '-ac', '2',
                    '-threads', '2', '-f', 'mpegts', '-'
                ],
                stdout=PIPE
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
            stdout=PIPE
        )
        try:
            # playout to rtmp
            playout = Popen(
                [
                    'ffmpeg', '-v', 'info', '-hide_banner', '-nostats', '-re',
                    '-fflags', '+igndts', '-thread_queue_size', '512',
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
                stdin=mbuffer.stdout
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
