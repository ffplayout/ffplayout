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
from xml.dom import minidom
from xml.parsers.expat import ExpatError


# get different time informations
def cur_ts(time_value, day):
    start_clock = datetime.now().strftime('%H:%M:%S')
    start_h, start_m, start_s = re.split(':', start_clock)
    time_in_sec = int(start_h) * 3600 + int(start_m) * 60 + int(start_s)

    if time_value == 't_hour':
        return start_h
    elif time_value == 't_full':
        return time_in_sec
    elif time_value == 't_date':
        t_from_cfg = int(cfg.get('PLAYLIST', 'day_start'))
        if int(start_h) < t_from_cfg and day != 'today':
            yesterday = date.today() - timedelta(1)
            list_date = yesterday.strftime('%Y-%m-%d')
        else:
            list_date = datetime.now().strftime('%Y-%m-%d')

        return list_date

# ------------------------------------------------------------------------------
# read values from config file
# ------------------------------------------------------------------------------


# read config
cfg = configparser.ConfigParser()
cfg.read("/etc/ffplayout/ffplayout.conf")


class _mail:
    server = cfg.get('MAIL', 'smpt_server')
    port = cfg.get('MAIL', 'smpt_port')
    s_addr = cfg.get('MAIL', 'sender_addr')
    s_pass = cfg.get('MAIL', 'sender_pass')
    recip = cfg.get('MAIL', 'recipient')


class _pre_comp:
    w = cfg.get('PRE_COMPRESS', 'width')
    h = cfg.get('PRE_COMPRESS', 'height')
    aspect = float(w) / float(h)
    fps = cfg.get('PRE_COMPRESS', 'fps')
    v_bitrate = cfg.get('PRE_COMPRESS', 'v_bitrate')
    v_bufsize = int(v_bitrate) / 2
    a_bitrate = cfg.get('PRE_COMPRESS', 'a_bitrate')
    a_sample = cfg.get('PRE_COMPRESS', 'a_sample')


class _playlist:
    path = cfg.get('PLAYLIST', 'playlist_path')
    start = int(cfg.get('PLAYLIST', 'day_start'))

    if cur_ts('t_full', '0') > 0 and cur_ts('t_full', '0') < start:
        start += 86400


class _buffer:
    length = cfg.get('BUFFER', 'buffer_length')
    cli = cfg.get('BUFFER', 'buffer_cli')
    cmd = literal_eval(cfg.get('BUFFER', 'buffer_cmd'))


class _playout:
    name = cfg.get('OUT', 'service_name')
    provider = cfg.get('OUT', 'service_provider')
    out_addr = cfg.get('OUT', 'out_addr')

    # set logo filtergraph
    if Path(cfg.get('OUT', 'logo')).is_file():
        logo_path = ['-thread_queue_size', '512', '-i', cfg.get('OUT', 'logo')]
        logo_graph = [
            '-filter_complex', '[0:v][1:v]' + cfg.get('OUT', 'logo_o') + '[o]',
            '-map', '[o]', '-map', '0:a'
        ]
    else:
        logo_path = []
        logo_graph = []
    post_comp_video = literal_eval(cfg.get('OUT', 'post_comp_video'))
    post_comp_audio = literal_eval(cfg.get('OUT', 'post_comp_audio'))
    post_comp_extra = literal_eval(cfg.get('OUT', 'post_comp_extra'))


# ------------------------------------------------------------------------------
# global functions
# ------------------------------------------------------------------------------

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
    total_size = (int(_pre_comp.v_bitrate) + int(_pre_comp.a_bitrate)) * \
        int(_buffer.length)

    return int(total_size)


# check if processes a well
def check_process(watch_proc, terminate_proc):
    while True:
        sleep(4)
        if watch_proc.poll() is not None:
            terminate_proc.terminate()
            break


# check if path exist,
# when not send email and generate blackclip
def check_path(f_o_l, in_file, duration, seek_t):
    in_path = Path(in_file)

    if f_o_l == 'list':
        error_message = 'Plylist does not exist:'
    elif f_o_l == 'file':
        error_message = 'File does not exist:'
    elif f_o_l == 'dummy_l':
        error_message = 'XML Playlist is not valid!'

    if not in_path.is_file() or f_o_l == 'dummy_l' or f_o_l == 'dummy_p':
        if f_o_l != 'dummy_p':
            send_mail(error_message, in_path)

        out_path = [
            '-f', 'lavfi', '-i',
            'color=s={}x{}:d={}'.format(
                _pre_comp.w, _pre_comp.h, duration
            ),
            '-f', 'lavfi', '-i', 'anullsrc=r=' + _pre_comp.a_sample,
            '-shortest'
        ]
    else:
        if float(seek_t) > 0.00:
            out_path = [
                '-ss', str(seek_t), '-i', in_file,
                '-vf', 'fade=in:st=0:d=0.5',
                '-af', 'afade=in:st=0:d=0.5'
            ]
        else:
            out_path = ['-i', in_file]

    return out_path


# ------------------------------------------------------------------------------
# main functions
# ------------------------------------------------------------------------------

# read values from xml playlist
def get_from_playlist(last_time, list_date, seek_in_clip):
    # path to current playlist
    l_y, l_m, l_d = re.split('-', list_date)
    c_p = '{}/{}/{}/{}.xml'.format(_playlist.path, l_y, l_m, list_date)

    src_cmd = check_path('list', c_p, 300, 0.00)

    if '-shortest' in src_cmd:
        clip_start = last_time

    else:
        try:
            xmldoc = minidom.parse(c_p)
        except ExpatError:
            src_cmd = check_path('dummy_l', c_p, 300, 0.00)

            return src_cmd, last_time, list_date, seek_in_clip

        clip_ls = xmldoc.getElementsByTagName('video')

        for i in range(len(clip_ls)):
            clip_start = re.sub(
                '[a-z=]', '', clip_ls[i].attributes['clipBegin'].value
            )
            clip_dur = re.sub('s', '', clip_ls[i].attributes['dur'].value)
            clip_path = clip_ls[i].attributes['src'].value

            # last clip in playlist
            if i == len(clip_ls) - 1:
                # last clip can be a filler
                # so we get the IN point and calculate the new duration
                # if the new duration is smaller then 6 sec put a blank clip
                clip_in = re.sub('s', '', clip_ls[i].attributes['in'].value)
                tmp_dur = float(clip_dur) - float(clip_in)

                if tmp_dur > 6.00:
                    src_cmd = check_path('file', clip_path, clip_dur, clip_in)
                elif tmp_dur > 1.00:
                    src_cmd = check_path('dummy_c', clip_path, tmp_dur, 0.00)
                else:
                    src_cmd = check_path('dummy_c', clip_path, 1, 0.00)

                clip_start = _playlist.start * 3600 - 5
                list_date = cur_ts('t_date', 'today')
                get_time = cur_ts('t_full', '0')

                # check if we are in time
                if int(get_time) > int(clip_start) + 10:
                    send_mail('we are out of time...:', get_time)

            # all other clips in playlist
            elif seek_in_clip is True:
                # first time we end up here
                if float(last_time) < float(clip_start) + float(clip_dur):
                    # calculate seek time
                    seek_t = float(last_time) - float(clip_start)
                    clip_len = float(clip_dur) - seek_t

                    src_cmd = check_path('file', clip_path, clip_len, seek_t)

                    seek_in_clip = False
                    break
            else:
                if float(last_time) < float(clip_start):
                    src_cmd = check_path('file', clip_path, clip_dur, 0.00)
                    break

    return src_cmd, clip_start, list_date, seek_in_clip


# independent thread for clip preparation
def play_clips(out_file):
    last_time = cur_ts('t_full', '0')
    list_date = cur_ts('t_date', '0')
    seek_in_clip = True

    # infinit loop
    # send current file from xml playlist to stdin from buffer
    while True:
        try:
            src_cmd, last_time, list_date, seek_in_clip = get_from_playlist(
                last_time, list_date, seek_in_clip
            )

            # tm_str = str(timedelta(seconds=int(float(last_time))))
            # print('[{}] current play command:\n{}\n'.format(tm_str, src_cmd))

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
                stdout=PIPE,
                stderr=PIPE
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
                list(_playout.logo_path) +
                list(_playout.logo_graph) +
                list(_playout.post_comp_video) +
                list(_playout.post_comp_audio) +
                [
                    '-metadata', 'service_name=' + _playout.name,
                    '-metadata', 'service_provider=' + _playout.provider,
                    '-metadata', 'year=' + cur_ts('t_date', 'today')
                ] +
                list(_playout.post_comp_extra) +
                [
                    _playout.out_addr
                ],
                stdin=mbuffer.stdout,
                stdout=PIPE
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
