import json
import os
from datetime import date
from platform import uname
from subprocess import PIPE, STDOUT, run
from time import sleep

import psutil

import yaml
import zmq
from apps.api_player.models import GuiSettings
from django.conf import settings
from natsort import natsorted
from pymediainfo import MediaInfo


def read_yaml():
    setting = GuiSettings.objects.filter(id=1).values()
    if setting:
        config = setting[0]

        if config and os.path.isfile(config['playout_config']):
            with open(config['playout_config'], 'r') as config_file:
                return yaml.safe_load(config_file)


def write_yaml(data):
    config = GuiSettings.objects.filter(id=1).values()[0]

    if os.path.isfile(config['playout_config']):
        with open(config['playout_config'], 'w') as outfile:
            yaml.dump(data, outfile, default_flow_style=False,
                      sort_keys=False, indent=4)


def read_json(date):
    config = read_yaml()['playlist']['path']
    y, m, d = date.split('-')
    input = os.path.join(config, y, m, '{}.json'.format(date))
    if os.path.isfile(input):
        with open(input, 'r') as playlist:
            return json.load(playlist)


def write_json(data):
    config = read_yaml()['playlist']['path']
    y, m, d = data['date'].split('-')
    output = os.path.join(config, y, m, '{}.json'.format(data['date']))
    with open(output, "w") as outfile:
        json.dump(data, outfile, indent=4)


def read_log(type, _date):
    config = read_yaml()
    log_path = config['logging']['log_path']

    if _date == date.today():
        log_file = os.path.join(log_path, '{}.log'.format(type))
    else:
        log_file = os.path.join(log_path, '{}.log.{}'.format(type, _date))

    if os.path.isfile(log_file):
        with open(log_file, 'r') as log:
            return log.read().strip()


def send_message(data):
    config = read_yaml()
    address, port = config['text']['bind_address'].split(':')

    context = zmq.Context(1)
    client = context.socket(zmq.REQ)
    client.connect('tcp://{}:{}'.format(address, port))

    poll = zmq.Poller()
    poll.register(client, zmq.POLLIN)

    request = ''
    reply_msg = ''

    for key, value in data.items():
        request += "{}='{}':".format(key, value)

    request = "{} reinit {}".format(settings.DRAW_TEXT_NODE, request.rstrip(':'))

    client.send_string(request)

    socks = dict(poll.poll(settings.REQUEST_TIMEOUT))

    if socks.get(client) == zmq.POLLIN:
        reply = client.recv()

        if reply and reply.decode() == '0 Success':
            reply_msg = reply.decode()
        else:
            reply_msg = reply.decode()
    else:
        reply_msg = 'No response from server'

    client.setsockopt(zmq.LINGER, 0)
    client.close()
    poll.unregister(client)

    context.term()
    return {'Success': reply_msg}


def sizeof_fmt(num, suffix='B'):
    for unit in ['', 'Ki', 'Mi', 'Gi', 'Ti', 'Pi', 'Ei', 'Zi']:
        if abs(num) < 1024.0:
            return "%3.1f%s%s" % (num, unit, suffix)
        num /= 1024.0
    return "%.1f%s%s" % (num, 'Yi', suffix)


class PlayoutService:
    def __init__(self):
        self.service = ['ffplayout-engine.service']
        self.cmd = ['sudo', '/bin/systemctl']
        self.proc = None

    def run_cmd(self):
        self.proc = run(self.cmd + self.service, stdout=PIPE, stderr=STDOUT,
                        encoding="utf-8").stdout

    def start(self):
        self.cmd.append('start')
        self.run_cmd()

    def stop(self):
        self.cmd.append('stop')
        self.run_cmd()

    def reload(self):
        self.cmd.append('reload')
        self.run_cmd()

    def restart(self):
        self.cmd.append('restart')
        self.run_cmd()

    def status(self):
        self.cmd.append('is-active')
        self.run_cmd()

        return self.proc.replace('\n', '')

    def log(self):
        self.cmd = ['sudo', '/bin/journalctl', '-n', '1000', '-u']

        self.run_cmd()

        return self.proc


class SystemStats:
    def __init__(self):
        settings = GuiSettings.objects.filter(id=1).values()
        self.config = settings[0] if settings else []

    def all(self):
        if self.config:
            return {
                **self.system(),
                **self.cpu(), **self.ram(), **self.swap(),
                **self.disk(), **self.net(), **self.net_speed()
            }

    def system(self):
        return {
            'system': uname().system,
            'node': uname().node,
            'machine': uname().machine
        }

    def cpu(self):
        return {
            'cpu_usage': psutil.cpu_percent(interval=1),
            'cpu_load': list(psutil.getloadavg())
            }

    def ram(self):
        mem = psutil.virtual_memory()
        return {
            'ram_total': [mem.total, sizeof_fmt(mem.total)],
            'ram_used': [mem.used, sizeof_fmt(mem.used)],
            'ram_free': [mem.free, sizeof_fmt(mem.free)],
            'ram_cached': [mem.cached, sizeof_fmt(mem.cached)]
        }

    def swap(self):
        swap = psutil.swap_memory()
        return {
            'swap_total': [swap.total, sizeof_fmt(swap.total)],
            'swap_used': [swap.used, sizeof_fmt(swap.used)],
            'swap_free': [swap.free, sizeof_fmt(swap.free)]
        }

    def disk(self):
        if 'media_disk' in self.config and self.config['media_disk']:
            root = psutil.disk_usage(self.config['media_disk'])
            return {
                'disk_total': [root.total, sizeof_fmt(root.total)],
                'disk_used': [root.used, sizeof_fmt(root.used)],
                'disk_free': [root.free, sizeof_fmt(root.free)]
            }

    def net(self):
        net = psutil.net_io_counters()
        return {
            'net_send': [net.bytes_sent, sizeof_fmt(net.bytes_sent)],
            'net_recv': [net.bytes_recv, sizeof_fmt(net.bytes_recv)],
            'net_errin': net.errin,
            'net_errout': net.errout
        }

    def net_speed(self):
        net = psutil.net_if_stats()

        if not 'net_interface' in self.config or \
                not self.config['net_interface']:
            return

        if self.config['net_interface'] not in net:
            return {
                'net_speed_send': 'no network interface set!',
                'net_speed_recv': 'no network interface set!'
            }

        net = psutil.net_io_counters(pernic=True)[self.config['net_interface']]

        send_start = net.bytes_sent
        recv_start = net.bytes_recv

        sleep(1)

        net = psutil.net_io_counters(pernic=True)[self.config['net_interface']]

        send_end = net.bytes_sent
        recv_end = net.bytes_recv

        send_sec = send_end - send_start
        recv_sec = recv_end - recv_start

        return {
            'net_speed_send': [send_sec, sizeof_fmt(send_sec)],
            'net_speed_recv': [recv_sec, sizeof_fmt(recv_sec)]
        }


def get_media_path(extensions, dir=None):
    config = read_yaml()
    extensions = extensions.split(' ')
    playout_extensions = config['storage']['extensions']
    gui_extensions = [x for x in extensions if x not in playout_extensions]
    media_path = config['storage']['path'].replace('\\', '/').rstrip('/')
    media_dir = media_path.split('/')[-1]
    media_root = os.path.dirname(media_path)
    if not dir:
        dir = media_path
    else:
        if '/..' in dir:
            # remove last folder to navigate in upper directory
            dir = '/'.join(dir.split('/')[:-2])

        dir = dir.lstrip('/')

        if dir.startswith(media_dir):
            dir = dir[len(media_dir):]

        dir = os.path.join(
            media_root, media_dir, os.path.abspath('/' + dir).strip('/'))

    for root, dirs, files in os.walk(dir, topdown=True):
        root = root.rstrip('/')
        media_files = []

        for file in files:
            ext = os.path.splitext(file)[1]
            if ext in playout_extensions:
                media_info = MediaInfo.parse(os.path.join(root, file))
                duration = 0
                for track in media_info.tracks:
                    if track.track_type == 'General':
                        try:
                            duration = float(
                                track.to_data()["duration"]) / 1000
                            break
                        except KeyError:
                            pass
                media_files.append({'file': file, 'duration': duration})
            elif ext in gui_extensions:
                media_files.append({'file': file, 'duration': ''})

        dirs = natsorted(dirs)

        if root != media_path:
            dirs.insert(0, '..')

        if not dirs:
            dirs = ['..']

        if root.startswith(media_root):
            root = root[len(media_root):]

        return [root, dirs, natsorted(media_files, key=lambda x: x['file'])]
