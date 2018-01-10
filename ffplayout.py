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
import re
import smtplib
from ast import literal_eval
from datetime import datetime, date, timedelta
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from pathlib import Path
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

_pre_comp = SimpleNamespace(
    w=cfg.getint('PRE_COMPRESS', 'width'),
    h=cfg.getint('PRE_COMPRESS', 'height'),
    aspect=cfg.getfloat('PRE_COMPRESS', 'width') /
    cfg.getfloat('PRE_COMPRESS', 'height'),
    fps=cfg.getint('PRE_COMPRESS', 'fps'),
    v_bitrate=cfg.getint('PRE_COMPRESS', 'v_bitrate'),
    v_bufsize=cfg.getint('PRE_COMPRESS', 'v_bitrate') / 2,
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
if Path(cfg.get('OUT', 'logo')).is_file():
    _playout.logo = ['-thread_queue_size', '512', '-i', cfg.get('OUT', 'logo')]
    _playout.filter = [
        '-filter_complex', '[0:v][1:v]' + cfg.get('OUT', 'logo_o') + '[o]',
        '-map', '[o]', '-map', '0:a'
    ]
else:
    _playout.logo = []
    _playout.filter = []


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
def send_mail(message, path):
    if _mail.recip:
        msg = MIMEMultipart()
        msg['From'] = _mail.s_addr
        msg['To'] = _mail.recip
        msg['Subject'] = "Playout Error"
        msg.attach(MIMEText('{}\n{}\n'.format(message, path), 'plain'))
        text = msg.as_string()

        server = smtplib.SMTP(_mail.server, int(_mail.port))
        server.starttls()
        server.login(_mail.s_addr, _mail.s_pass)
        server.sendmail(_mail.s_addr, _mail.recip, text)
        server.quit()
    else:
        print('{}\n{}\n'.format(message, path))


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
    in_path = Path(in_file)

    if in_path.is_file():
        return True
    else:
        send_mail('File does not exist ({}):'.format(get_time('str')), in_path)
        return False


def seek_in_clip(in_file, seek_t):
    return [
        '-ss', str(seek_t), '-i', in_file,
        '-vf', 'fade=in:st=0:d=0.5', '-af', 'afade=in:st=0:d=0.5'
    ]


def gen_dummy(duration):
    return [
        '-f', 'lavfi', '-i',
        'color=s={}x{}:d={}'.format(
            _pre_comp.w, _pre_comp.h, duration
        ),
        '-f', 'lavfi', '-i', 'anullsrc=r=' + _pre_comp.a_sample, '-shortest'
    ]


# last clip can be a filler
# so we get the IN point and calculate the new duration
# if the new duration is smaller then 6 sec put a blank clip
def prepare_last_clip(in_node, start):
    clip_path = in_node.get('src')
    clip_len = float(in_node.get('dur').rstrip('s'))
    clip_in = float(in_node.get('in').rstrip('s'))
    tmp_dur = clip_len - clip_in
    current_time = get_time('full_sec')

    # check if we are in time
    if get_time('full_sec') > start + 10:
        send_mail('we are out of time...:', current_time)

    if tmp_dur > 6.00:
        if check_file_exist():
            src_cmd = seek_in_clip(clip_path, clip_in)
        else:
            src_cmd = gen_dummy(tmp_dur)
    elif tmp_dur > 1.00:
        src_cmd = gen_dummy(tmp_dur)
    else:
        src_cmd = 'next'

    return src_cmd


# ------------------------------------------------------------------------------
# main functions
# ------------------------------------------------------------------------------

# read values from xml playlist
def get_from_playlist(last_time, list_date, seek):
    # path to current playlist
    year, month, day = re.split('-', list_date)
    xml_path = '{}/{}/{}/{}.xml'.format(
        _playlist.path, year, month, list_date
    )

    if check_file_exist(xml_path):
        xml_root = ET.parse(xml_path).getroot()
        clip_nodes = xml_root.findall('body/video')

        # all clips in playlist except last one
        for clip_node in clip_nodes[:-1]:
            clip_path = clip_node.get('src')
            clip_start = float(clip_node.get('clipBegin').rstrip('s'))
            clip_len = float(clip_node.get('dur').rstrip('s'))

            if seek:
                # first time we end up here
                if last_time < clip_start + clip_len:
                    # calculate seek time
                    seek_t = last_time - clip_start

                    if check_file_exist(clip_path):
                        src_cmd = seek_in_clip(clip_path, seek_t)
                    else:
                        src_cmd = gen_dummy(clip_len - seek_t)

                    seek = False
                    break
            else:
                if last_time < clip_start:
                    if check_file_exist(clip_path):
                        src_cmd = ['-i', clip_path]
                    else:
                        src_cmd = gen_dummy(clip_len)

                    break
        # last clip in playlist
        else:
            clip_start = float(_playlist.start * 3600 - 5)
            src_cmd = prepare_last_clip(clip_nodes[-1], clip_start)
            list_date = get_date(False)

    else:
        src_cmd = gen_dummy(300)
        return src_cmd, last_time + 300, list_date, seek

    return src_cmd, clip_start, get_date(True), seek


# independent thread for clip preparation
def play_clips(out_file):
    if get_time('full_sec') > 0 and \
            get_time('full_sec') < _playlist.start * 3600:
        last_time = float(get_time('full_sec') + 86400)
    else:
        last_time = float(get_time('full_sec'))

    list_date = get_date(True)
    seek = True

    # infinit loop
    # send current file from xml playlist to stdin from buffer
    while True:
        try:
            src_cmd, last_time, list_date, seek = get_from_playlist(
                last_time, list_date, seek
            )

            if src_cmd == 'next':
                src_cmd, last_time, list_date, seek = get_from_playlist(
                    last_time, list_date, seek
                )

            if last_time > 86400:
                tm_str = str(timedelta(seconds=int(last_time - 86400)))
            else:
                tm_str = str(timedelta(seconds=int(last_time)))

            print('[{}] current play command:\n{}\n'.format(tm_str, src_cmd))

            filePiper = Popen(
                [
                    'ffmpeg', '-v', 'error', '-hide_banner', '-nostats'
                ] + src_cmd +
                [
                    '-s', '{}x{}'.format(_pre_comp.w, _pre_comp.h),
                    '-aspect', str(_pre_comp.aspect),
                    '-pix_fmt', 'yuv420p', '-r', str(_pre_comp.fps),
                    '-c:v', 'mpeg2video', '-g', '12', '-bf', '2',
                    '-b:v', '{}k'.format(_pre_comp.v_bitrate),
                    '-minrate', '{}k'.format(_pre_comp.v_bitrate),
                    '-maxrate', '{}k'.format(_pre_comp.v_bitrate),
                    '-bufsize', '{}k'.format(_pre_comp.v_bufsize),
                    '-c:a', 'mp2', '-b:a', '{}k'.format(_pre_comp.a_bitrate),
                    '-ar', str(_pre_comp.a_sample), '-ac', '2', '-f', 'mpegts',
                    '-threads', '2', '-'
                ],
                stdout=PIPE
            )

            copyfileobj(filePiper.stdout, out_file)
        finally:
            filePiper.wait()


def main():
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
                    'ffmpeg', '-v', 'error', '-hide_banner', '-re',
                    '-fflags', '+igndts', '-i', 'pipe:0', '-fflags', '+genpts'
                ] +
                list(_playout.logo) +
                list(_playout.filter) +
                list(_playout.post_comp_video) +
                list(_playout.post_comp_audio) +
                [
                    '-metadata', 'service_name=' + _playout.name,
                    '-metadata', 'service_provider=' + _playout.provider,
                    '-metadata', 'year=' + get_date(False)
                ] +
                list(_playout.post_comp_extra) +
                [
                    _playout.out_addr
                ],
                stdin=mbuffer.stdout
            )

            play_thread = Thread(
                name='play_clips', target=play_clips, args=(mbuffer.stdin,)
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
