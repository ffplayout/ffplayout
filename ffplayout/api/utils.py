import configparser
import os
from platform import uname
from time import sleep

from django.conf import settings

import psutil
from natsort import natsorted


def sizeof_fmt(num, suffix='B'):
    for unit in ['', 'Ki', 'Mi', 'Gi', 'Ti', 'Pi', 'Ei', 'Zi']:
        if abs(num) < 1024.0:
            return "%3.1f%s%s" % (num, unit, suffix)
        num /= 1024.0
    return "%.1f%s%s" % (num, 'Yi', suffix)


class IniParser(configparser.ConfigParser):
    """
    config output as json
    """

    def as_dict(self):
        d = dict(self._sections)
        for k in d:
            d[k] = dict(self._defaults, **d[k])
            d[k].pop('__name__', None)
        return d


class SystemStats:
    def __init__(self):
        pass

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
        root = psutil.disk_usage(settings.MEDIA_DISK)
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

        if settings.NET_INTERFACE not in net:
            return {
                'net_speed_send': 'no network interface set!',
                'net_speed_recv': 'no network interface set!'
            }

        net = psutil.net_io_counters(pernic=True)[settings.NET_INTERFACE]

        send_start = net.bytes_sent
        recv_start = net.bytes_recv

        sleep(1)

        net = psutil.net_io_counters(pernic=True)[settings.NET_INTERFACE]

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
    root = os.path.dirname(settings.MEDIA_FOLDER)
    return path.lstrip(root)


def get_media_path(dir=None):
    if not dir:
        if not os.path.isdir(settings.MEDIA_FOLDER):
            return ''
        dir = settings.MEDIA_FOLDER
    else:
        if '/..' in dir:
            dir = '/'.join(dir.split('/')[:-2])
            # prevent deeper access
            dir = dir.replace('/../', '/')

        dir = os.path.join(os.path.dirname(settings.MEDIA_FOLDER), dir)
    for root, dirs, files in os.walk(dir, topdown=True):
        media_files = []

        for file in files:
            if os.path.splitext(file)[1] in settings.MEDIA_EXTENSIONS:
                media_files.append(file)

        dirs = natsorted(dirs)

        if root != settings.MEDIA_FOLDER:
            dirs.insert(0, '..')

        if not dirs:
            dirs = ['..']

        return [set_root(root), dirs, natsorted(media_files)]


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
