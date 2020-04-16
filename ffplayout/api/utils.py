import json
import os
from platform import uname
from time import sleep

import psutil
import yaml
from pymediainfo import MediaInfo

from api.models import GuiSettings

from natsort import natsorted


def read_yaml():
    config = GuiSettings.objects.filter(id=1).values()[0]

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


def sizeof_fmt(num, suffix='B'):
    for unit in ['', 'Ki', 'Mi', 'Gi', 'Ti', 'Pi', 'Ei', 'Zi']:
        if abs(num) < 1024.0:
            return "%3.1f%s%s" % (num, unit, suffix)
        num /= 1024.0
    return "%.1f%s%s" % (num, 'Yi', suffix)


class SystemStats:
    def __init__(self):
        self.config = GuiSettings.objects.filter(id=1).values()[0]

    def all(self):
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


def set_root(path):
    # prevent access to root file system
    dir = os.path.dirname(
        read_yaml()['storage']['path'].replace('\\', '/').rstrip('/'))

    return path.replace(dir, '').strip('/')


def get_media_path(extensions, dir=None):
    config = read_yaml()
    extensions = extensions.split(' ')
    playout_extensions = config['storage']['extensions']
    gui_extensions = [x for x in extensions if x not in playout_extensions]
    media_dir = config['storage']['path'].replace('\\', '/').rstrip('/')
    if not dir:
        if not os.path.isdir(media_dir):
            return ''
        dir = media_dir
    else:
        if '/..' in dir:
            dir = '/'.join(dir.split('/')[:-2])

        dir = os.path.join(os.path.dirname(media_dir),
                           os.path.abspath('/' + dir).strip('/'))
    for root, dirs, files in os.walk(dir, topdown=True):
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

        if root != media_dir:
            dirs.insert(0, '..')

        if not dirs:
            dirs = ['..']

        return [set_root(root), dirs, natsorted(media_files,
                                                key=lambda x: x['file'])]


if __name__ == '__main__':
    result = hasattr(SystemStats(), 'system')
    print(result)
    exit()
    print('CPU: ', SystemStats.cpu())
    print('RAM: ', SystemStats.ram())
    print('SWAP: ', SystemStats.swap())
    print('DISK: ', SystemStats.disk())
    print('NET: ', SystemStats.net())
    print('SPEED: ', SystemStats.net_speed())
