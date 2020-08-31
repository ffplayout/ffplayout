#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import json
from argparse import ArgumentParser

import requests
import urllib3
from pymediainfo import MediaInfo

urllib3.disable_warnings()

# ------------------------------------------------------------------------------
# argument parsing
# ------------------------------------------------------------------------------

stdin_parser = ArgumentParser(
    description='Convert text file to playlist and send it to the API')

stdin_parser.add_argument(
    '-u', '--user', help='API user', required=True
)

stdin_parser.add_argument(
    '-p', '--password', help='API password', required=True
)

stdin_parser.add_argument(
    '-c', '--channel', help='Channel name'
)

stdin_parser.add_argument(
    '--url', help='the url from the ffplayout API', required=True
)

stdin_parser.add_argument(
    '-d', '--date', help='date from target playlist, in YYYY-MM-DD',
    required=True
)

stdin_parser.add_argument(
    '-f', '--file', help=('text file with clips, '
                          'paths must match the paths on ffplayout'),
    required=True
)

stdin_args = stdin_parser.parse_args()


def auth():
    login = {'username': stdin_args.user,
             'password': stdin_args.password}

    req = requests.post(
        '{}/auth/token/'.format(stdin_args.url), data=login)
    token = req.json()
    return token['access']


def get_video_duration(clip):
    """
    return video duration from container
    """
    media_info = MediaInfo.parse(clip)
    duration = 0
    for track in media_info.tracks:
        if track.track_type == 'General':
            try:
                duration = float(
                    track.to_data()["duration"]) / 1000
                break
            except KeyError:
                pass

    return duration


def gen_playlist():
    json_data = {
        'channel': stdin_args.channel if stdin_args.channel else 'Channel 1',
        'date': stdin_args.date,
        'program': []
    }

    with open(stdin_args.file, 'r') as content:
        for line in content:
            src = line.strip().strip('"').strip("'")
            duration = get_video_duration(src)
            json_data['program'].append({
                'in': 0,
                'out': duration,
                'duration': duration,
                'source': src
            })

    return json_data


if __name__ == '__main__':
    playlist = gen_playlist()

    req = requests.post(
        '{}/api/player/playlist/'.format(stdin_args.url),
        data=json.dumps({'data': playlist}),
        headers={'Authorization': 'Bearer {}'.format(auth()),
                 'content-type': 'application/json'})

    if req.status_code == 201:
        print('Save remote playlist from done...')
    else:
        print(req.json()['detail'])
