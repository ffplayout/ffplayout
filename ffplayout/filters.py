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

import math
import os

from .utils import _pre_comp


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
        filter_chain.append('fps={}'.format(_pre_comp.fps))

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
        opacity = 'format=rgba,scale={},colorchannelmixer=aa={}'.format(
            _pre_comp.logo_scale, _pre_comp.logo_opacity)
        loop = 'loop=loop=-1:size=1:start=0'
        logo_chain.append(
            'movie={},{},{}'.format(_pre_comp.logo, loop, opacity))
        if ad_last:
            logo_chain.append('fade=in:st=0:d=1.0:alpha=1')
        if ad_next:
            logo_chain.append('fade=out:st={}:d=1.0:alpha=1'.format(
                duration - 1))

        logo_filter = '{}[l];[v][l]{}:shortest=1[logo]'.format(
            ','.join(logo_chain), _pre_comp.logo_filter)

    return logo_filter


def add_audio(probe, duration, msg):
    """
    when clip has no audio we generate an audio line
    """
    line = []

    if not probe.audio:
        msg.warning('Clip "{}" has no audio!'.format(probe.src))
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


def build_filtergraph(duration, seek, out, ad, ad_last, ad_next, probe, msg):
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

        audio_chain += add_audio(probe, out - seek, msg)

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
