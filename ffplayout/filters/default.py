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
This module prepare all ffmpeg filters.
This is mainly for unify clips to have a unique output.
"""

import math
import os
import re
from glob import glob
from pydoc import locate

from ffplayout.utils import (is_advertisement, lower_third, messenger, pre,
                             sync_op)

# ------------------------------------------------------------------------------
# building filters,
# when is needed add individual filters to match output format
# ------------------------------------------------------------------------------


def text_filter():
    """
    add drawtext filter for lower thirds messages
    """
    filter_chain = []
    font = ''

    if lower_third.add_text and lower_third.over_pre:
        if lower_third.fontfile and os.path.isfile(lower_third.fontfile):
            font = f":fontfile='{lower_third.fontfile}'"
        filter_chain = [
            "null,zmq=b=tcp\\\\://'{}',drawtext=text=''{}".format(
                lower_third.address.replace(':', '\\:'), font)]

    return filter_chain


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
                        pre.aspect, abs_tol=0.03):
        if probe.video[0]['aspect'] < pre.aspect:
            filter_chain.append(
                f'pad=ih*{pre.w}/{pre.h}/sar:ih:(ow-iw)/2:(oh-ih)/2')
        elif probe.video[0]['aspect'] > pre.aspect:
            filter_chain.append(
                f'pad=iw:iw*{pre.h}/{pre.w}/sar:(ow-iw)/2:(oh-ih)/2')

    return filter_chain


def fps_filter(probe):
    """
    changing frame rate
    """
    filter_chain = []

    if probe.video[0]['fps'] != pre.fps:
        filter_chain.append(f'fps={pre.fps}')

    return filter_chain


def scale_filter(probe):
    """
    if target resolution is different to source add scale filter,
    apply also an aspect filter, when is different
    """
    filter_chain = []

    if int(probe.video[0]['width']) != pre.w or \
            int(probe.video[0]['height']) != pre.h:
        filter_chain.append(f'scale={pre.w}:{pre.h}')

    if not math.isclose(probe.video[0]['aspect'],
                        pre.aspect, abs_tol=0.03):
        filter_chain.append(f'setdar=dar={pre.aspect}')

    return filter_chain


def fade_filter(duration, seek, out, track=''):
    """
    fade in/out video, when is cutted at the begin or end
    """
    filter_chain = []

    if seek > 0.0:
        filter_chain.append(f'{track}fade=in:st=0:d=0.5')

    if out != duration and out - seek - 1.0 > 0:
        filter_chain.append(f'{track}fade=out:st={out - seek - 1.0}:d=1.0')

    return filter_chain


def overlay_filter(duration, advertisement, ad_last, ad_next):
    """
    overlay logo: when is an ad don't overlay,
    when ad is coming next fade logo out,
    when clip before was an ad fade logo in
    """
    logo_filter = '[v]null'
    scale = ''

    if pre.add_logo and os.path.isfile(pre.logo) and not advertisement:
        logo_chain = []
        if pre.logo_scale and \
                re.match(r'\d+:-?\d+', pre.logo_scale):
            scale = f'scale={pre.logo_scale},'
        logo_extras = (f'format=rgba,{scale}'
                       f'colorchannelmixer=aa={pre.logo_opacity}')
        loop = 'loop=loop=-1:size=1:start=0'
        logo_chain.append(f'movie={pre.logo},{loop},{logo_extras}')
        if ad_last:
            logo_chain.append('fade=in:st=0:d=1.0:alpha=1')
        if ad_next:
            logo_chain.append(f'fade=out:st={duration - 1}:d=1.0:alpha=1')

        logo_filter = (f'{",".join(logo_chain)}[l];[v][l]'
                       f'{pre.logo_filter}:shortest=1')

    return logo_filter


def add_audio(probe, duration):
    """
    when clip has no audio we generate an audio line
    """
    line = []

    if not probe.audio:
        messenger.warning(f'Clip "{probe.src}" has no audio!')
        line = [(f'aevalsrc=0:channel_layout=stereo:duration={duration}:'
                 f'sample_rate={48000}')]

    return line


def add_loudnorm(probe):
    """
    add single pass loudnorm filter to audio line
    """
    loud_filter = []

    if probe.audio and pre.add_loudnorm:
        loud_filter = [
            f'loudnorm=I={pre.loud_i}:TP={pre.loud_tp}:LRA={pre.loud_lra}']

    return loud_filter


def extend_audio(probe, duration, target_duration):
    """
    check audio duration, is it shorter then clip duration - pad it
    """
    pad = []
    aud_dur = probe.audio[0].get('duration') if probe.audio else None

    if aud_dur and target_duration <= duration > float(aud_dur) + 0.1:
        pad.append(f'apad=whole_dur={target_duration}')

    return pad


def extend_video(probe, duration, target_duration):
    """
    check video duration, is it shorter then clip duration - pad it
    """
    pad = []
    vid_dur = probe.video[0].get('duration')

    if vid_dur and target_duration < duration > float(vid_dur) + 0.1:
        pad.append(
            f'tpad=stop_mode=add:stop_duration={duration - float(vid_dur)}')

    return pad


def realtime_filter(duration, track=''):
    """
    this realtime filter is important for HLS output to stay in sync
    """
    speed_filter = ''

    if sync_op.realtime:
        speed_filter = f',{track}realtime=speed=1'

        if sync_op.time_delta < 0:
            speed = duration / (duration + sync_op.time_delta)

            if speed < 1.1:
                speed_filter = f',{track}realtime=speed={speed}'

    return speed_filter


def split_filter(filter_type):
    """
    this filter splits the media input in multiple outputs,
    to be able to have different streaming/HLS outputs
    """
    map_node = []
    prefix = ''
    _filter = ''

    if filter_type == 'a':
        prefix = 'a'

    if pre.output_count > 1:
        for num in range(pre.output_count):
            map_node.append(f'[{filter_type}out{num + 1}]')

        _filter = f',{prefix}split={pre.output_count}{"".join(map_node)}'

    else:
        _filter = f'[{filter_type}out1]'

    return _filter


def custom_filter(filter_type, node):
    """
    read custom filters from filters folder
    """
    filter_dir = os.path.dirname(os.path.abspath(__file__))
    filters = []

    for filter_file in glob(os.path.join(filter_dir, f'{filter_type}_*')):
        filter_ = os.path.splitext(os.path.basename(filter_file))[0]
        filter_function = locate(f'ffplayout.filters.{filter_}.filter_link')
        link = filter_function(node)

        if link is not None:
            filters.append(link)

    return filters


def build_filtergraph(node, node_last, node_next):
    """
    build final filter graph, with video and audio chain
    """

    advertisement = is_advertisement(node)
    ad_last = is_advertisement(node_last)
    ad_next = is_advertisement(node_next)

    duration = node['duration']
    seek = node['seek']
    out = node['out']
    probe = node['probe']

    video_chain = []
    audio_chain = []

    if out > duration:
        seek = 0

    if probe and probe.video[0]:
        custom_v_filter = custom_filter('v', node)
        video_chain += text_filter() \
            + deinterlace_filter(probe) \
            + pad_filter(probe) \
            + fps_filter(probe) \
            + scale_filter(probe) \
            + extend_video(probe, duration, out - seek)
        if custom_v_filter:
            video_chain += custom_v_filter
        video_chain += fade_filter(duration, seek, out)

        audio_chain += add_audio(probe, out - seek)

        if not audio_chain:
            custom_a_filter = custom_filter('a', node)

            audio_chain += ['[0:a]anull'] \
                + add_loudnorm(probe) \
                + extend_audio(probe, duration, out - seek)
            if custom_a_filter:
                audio_chain += custom_a_filter
            audio_chain += fade_filter(duration, seek, out, 'a')

    if video_chain:
        video_filter = f'{",".join(video_chain)}[v]'
    else:
        video_filter = 'null[v]'

    logo_filter = overlay_filter(out - seek, advertisement, ad_last, ad_next)
    v_speed = realtime_filter(out - seek)
    v_split = split_filter('v')
    video_map = ['-map', '[vout1]']
    video_filter = [
        '-filter_complex',
        f'[0:v]{video_filter};{logo_filter}{v_speed}{v_split}']

    a_speed = realtime_filter(out - seek, 'a')
    a_split = split_filter('a')
    audio_map = ['-map', '[aout1]']
    audio_filter = [
        '-filter_complex', f'{",".join(audio_chain)}{a_speed}{a_split}']

    if probe and probe.video[0]:
        return video_filter + audio_filter + video_map + audio_map

    return video_filter + video_map + ['-map', '1:a']
