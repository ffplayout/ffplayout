#!/usr/bin/env python3
"""Send FLV timestamps verbatim over RTMP, including deliberate discontinuities.

FFmpeg remuxing can repair the deliberately non-monotonic audio timestamps.
This small publisher avoids that repair. Use only with a test live input.
"""
import argparse
import socket
import struct
import threading
import time
from pathlib import Path
from urllib.parse import urlsplit


def amf(value):
    if isinstance(value, str):
        data = value.encode()
        return b'\x02' + struct.pack('>H', len(data)) + data
    if isinstance(value, bool):
        return b'\x01' + bytes([value])
    if isinstance(value, (int, float)):
        return b'\x00' + struct.pack('>d', value)
    if value is None:
        return b'\x05'
    if isinstance(value, dict):
        return b'\x03' + b''.join(
            struct.pack('>H', len(k.encode())) + k.encode() + amf(v)
            for k, v in value.items()
        ) + b'\x00\x00\x09'
    raise TypeError(value)


class Publisher:
    def __init__(self, url):
        parsed = urlsplit(url)
        if parsed.scheme != 'rtmp':
            raise ValueError('Use an rtmp:// URL')
        self.app, separator, self.key = parsed.path.lstrip('/').partition('/')
        if not separator or not self.key:
            raise ValueError('URL must contain an application and stream key')
        self.tc_url = f'rtmp://{parsed.netloc}/{self.app}'
        self.sock = socket.create_connection((parsed.hostname, parsed.port or 1935), 15)
        self.read_chunk_size = 128
        self.write_chunk_size = 128
        self.channels = {}
        self.write_lock = threading.Lock()
        self.closed = threading.Event()

    def receive(self, count):
        result = bytearray()
        while len(result) < count:
            part = self.sock.recv(count - len(result))
            if not part:
                raise EOFError('RTMP listener closed the connection')
            result.extend(part)
        return bytes(result)

    def send(self, kind, payload, stream=0, timestamp=0, channel=3):
        extended = timestamp >= 0xffffff
        stamp = min(timestamp, 0xffffff).to_bytes(3, 'big')
        header = bytes([channel]) + stamp + len(payload).to_bytes(3, 'big')
        header += bytes([kind]) + struct.pack('<I', stream)
        extra = struct.pack('>I', timestamp) if extended else b''
        wire = header + extra + payload[:self.write_chunk_size]
        for offset in range(self.write_chunk_size, len(payload), self.write_chunk_size):
            wire += bytes([0xc0 | channel]) + extra + payload[offset:offset + self.write_chunk_size]
        with self.write_lock:
            self.sock.sendall(wire)

    def command(self, name, transaction, *values, stream=0):
        self.send(20, b''.join(amf(v) for v in (name, transaction, *values)), stream)

    def read_message(self):
        while True:
            basic = self.receive(1)[0]
            fmt, channel = basic >> 6, basic & 63
            if channel == 0:
                channel = 64 + self.receive(1)[0]
            elif channel == 1:
                channel = 64 + int.from_bytes(self.receive(2), 'little')
            state = self.channels.get(channel)
            if fmt == 0:
                header = self.receive(11)
                stamp = int.from_bytes(header[:3], 'big')
                state = dict(timestamp=stamp, delta=0, length=int.from_bytes(header[3:6], 'big'),
                             kind=header[6], stream=int.from_bytes(header[7:], 'little'),
                             extended=stamp == 0xffffff, data=bytearray())
                if state['extended']:
                    state['timestamp'] = int.from_bytes(self.receive(4), 'big')
                self.channels[channel] = state
            elif state is None:
                raise ValueError('RTMP continuation without a preceding header')
            elif fmt in (1, 2):
                header = self.receive(7 if fmt == 1 else 3)
                delta = int.from_bytes(header[:3], 'big')
                state['extended'] = delta == 0xffffff
                if state['extended']:
                    delta = int.from_bytes(self.receive(4), 'big')
                state['delta'] = delta
                state['timestamp'] += delta
                state['data'] = bytearray()
                if fmt == 1:
                    state['length'] = int.from_bytes(header[3:6], 'big')
                    state['kind'] = header[6]
            else:
                if not state['data']:
                    state['timestamp'] += state['delta']
                if state['extended']:
                    self.receive(4)
            remaining = state['length'] - len(state['data'])
            state['data'].extend(self.receive(min(self.read_chunk_size, remaining)))
            if len(state['data']) != state['length']:
                continue
            kind, data = state['kind'], bytes(state['data'])
            state['data'] = bytearray()
            if kind == 1:
                self.read_chunk_size = int.from_bytes(data, 'big') & 0x7fffffff
            elif kind == 4 and data[:2] == b'\x00\x06':
                self.send(4, b'\x00\x07' + data[2:], channel=2)
            return kind, data

    def wait_result(self, transaction):
        while True:
            kind, data = self.read_message()
            if kind != 20 or not data.startswith(amf('_result')):
                if kind == 20 and data.startswith(amf('_error')):
                    raise RuntimeError(f'RTMP command rejected: {data!r}')
                continue
            offset = len(amf('_result'))
            if data[offset:offset + 9] == amf(transaction):
                return data

    def connect(self):
        self.sock.sendall(b'\x03' + struct.pack('>II', int(time.time()), 0) + bytes(1528))
        handshake = self.receive(3073)
        if handshake[0] != 3:
            raise ValueError('Unsupported RTMP handshake')
        self.sock.sendall(handshake[1:1537])
        self.send(1, struct.pack('>I', 4096), channel=2)
        self.write_chunk_size = 4096
        self.command('connect', 1, {'app': self.app, 'type': 'nonprivate',
                     'tcUrl': self.tc_url, 'flashVer': 'FMLE/3.0', 'objectEncoding': 0})
        self.wait_result(1)
        self.command('releaseStream', 0, None, self.key)
        self.command('FCPublish', 0, None, self.key)
        self.command('createStream', 2, None)
        result = self.wait_result(2)
        if result[-9] != 0:
            raise ValueError('Invalid createStream response')
        self.stream = int(struct.unpack('>d', result[-8:])[0])
        self.command('publish', 0, None, self.key, 'live', stream=self.stream)
        while True:
            kind, data = self.read_message()
            if kind == 20 and b'NetStream.Publish.Start' in data:
                break
            if kind == 20 and any(code in data for code in (b'BadName', b'Denied', b'Failed')):
                raise RuntimeError(f'Publish rejected: {data!r}')
        self.sock.settimeout(None)
        threading.Thread(target=self.monitor, daemon=True).start()

    def monitor(self):
        try:
            while not self.closed.is_set():
                self.read_message()
        except (OSError, EOFError, ValueError):
            self.closed.set()


def tags(path):
    with path.open('rb') as source:
        header = source.read(9)
        if header[:3] != b'FLV':
            raise ValueError('Expected an FLV file')
        source.seek(int.from_bytes(header[5:9], 'big') + 4)
        while True:
            header = source.read(11)
            if not header:
                return
            if len(header) != 11:
                raise ValueError('Truncated FLV header')
            length = int.from_bytes(header[1:4], 'big')
            timestamp = int.from_bytes(header[4:7], 'big') | (header[7] << 24)
            payload = source.read(length)
            previous_size = source.read(4)
            if len(payload) != length or int.from_bytes(previous_size, 'big') != length + 11:
                raise ValueError('Truncated or invalid FLV tag')
            yield header[0], timestamp, payload


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('file', type=Path)
    parser.add_argument('url', help='ffplayout live ingest URL')
    parser.add_argument('--normal', action='store_true', help='Repair the outlier for a control run')
    parser.add_argument('--audio-batch-ms', type=int, default=200, help='Send audio in timestamp-preserving batches (default: 200 ms)')
    parser.add_argument('--duration', type=float, default=60.0, help='Test duration in seconds (default: 60)')
    parser.add_argument('--fast', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.duration <= 0 or args.audio_batch_ms < 0:
        parser.error('duration must be positive and audio-batch-ms must not be negative')
    publisher = Publisher(args.url)
    try:
        publisher.connect()
        print(f'Connected. Sending up to {args.duration:g} seconds of the test stream.', flush=True)
        start = time.monotonic()
        latest_video = schedule_ms = 0
        pending_audio = []
        next_audio_flush = args.audio_batch_ms
        for kind, timestamp, payload in tags(args.file):
            if kind not in (8, 9, 18):
                continue
            if kind == 9:
                latest_video = timestamp
            outlier = kind == 8 and timestamp > latest_video + 5000
            if outlier:
                action = 'Repairing' if args.normal else 'Injecting'
                print(f'{action} one audio timestamp outlier: {timestamp / 1000:.3f}s '
                      f'while video is at {latest_video / 1000:.3f}s', flush=True)
            pace_timestamp = latest_video if outlier else timestamp
            schedule_ms = max(schedule_ms, pace_timestamp)
            if schedule_ms >= args.duration * 1000:
                break
            if not args.fast:
                time.sleep(max(0, start + schedule_ms / 1000 - time.monotonic()))
            if publisher.closed.is_set():
                raise EOFError('Listener closed the connection during the test')
            if args.normal and outlier:
                timestamp -= 5377
            if kind == 8 and len(payload) > 1 and payload[1] == 1 and args.audio_batch_ms > 0:
                pending_audio.append((timestamp, payload))
            else:
                publisher.send(kind, payload, stream=publisher.stream, timestamp=timestamp,
                               channel={8: 5, 9: 6, 18: 4}[kind])
            if schedule_ms >= next_audio_flush:
                for audio_timestamp, audio_payload in pending_audio:
                    publisher.send(8, audio_payload, stream=publisher.stream,
                                   timestamp=audio_timestamp, channel=5)
                pending_audio.clear()
                next_audio_flush = schedule_ms + max(1, args.audio_batch_ms)
        for audio_timestamp, audio_payload in pending_audio:
            publisher.send(8, audio_payload, stream=publisher.stream,
                           timestamp=audio_timestamp, channel=5)
        print('Done.', flush=True)
    finally:
        publisher.closed.set()
        publisher.sock.close()


if __name__ == '__main__':
    main()
