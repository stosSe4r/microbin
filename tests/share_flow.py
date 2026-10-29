#!/usr/bin/env python3
"""Run against an isolated local process: python3 tests/share_flow.py [--browser]."""
import base64
import contextlib
import html
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('MICROBIN_BINARY', ROOT / 'target/debug/microbin')).resolve()
PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aX1cAAAAASUVORK5CYII=')


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args):
        return None


OPENER = urllib.request.build_opener(NoRedirect)


def request(url, data=None, headers=None):
    try:
        response = OPENER.open(urllib.request.Request(url, data=data, headers=headers or {}), timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, response.headers, response.read()


def upload(base, content='', files=(), privacy='public', auth=None):
    boundary = 'microbin-share-flow-check'
    fields = [('expiration', '1hour'), ('privacy', privacy), ('burn_after', '0'), ('content', content)]
    body = bytearray()
    for name, value in fields:
        body.extend(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode())
    for name, contents in files:
        body.extend(f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="{name}"\r\nContent-Type: application/octet-stream\r\n\r\n'.encode())
        body.extend(contents + b'\r\n')
    body.extend(f'--{boundary}--\r\n'.encode())
    headers = {'Content-Type': f'multipart/form-data; boundary={boundary}'}
    if auth:
        headers['Authorization'] = auth
    status, headers, _ = request(base + '/upload', bytes(body), headers)
    assert status == 302, (status, headers)
    return headers['Location'].rsplit('/', 1)[-1]


@contextlib.contextmanager
def server(**settings):
    with tempfile.TemporaryDirectory(prefix='microbin-check-') as directory:
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        env = {key: value for key, value in os.environ.items() if not key.startswith('MICROBIN_')}
        env.update({
            'MICROBIN_PORT': str(port), 'MICROBIN_BIND': '127.0.0.1',
            'MICROBIN_JSON_DB': 'true', 'MICROBIN_DATA_DIR': 'pasta_data',
            'MICROBIN_QR': 'true', 'MICROBIN_HASH_IDS': 'true',
            'MICROBIN_PUBLIC_PATH': base, 'MICROBIN_ETERNAL_PASTA': 'true',
            'MICROBIN_DISABLE_TELEMETRY': 'true', 'MICROBIN_DISABLE_UPDATE_CHECKING': 'true',
            'MICROBIN_ADMIN_USERNAME': '', 'MICROBIN_ADMIN_PASSWORD': '',
        })
        env.update(settings)
        with open(Path(directory) / 'server.log', 'wb') as log:
            process = subprocess.Popen([str(BINARY)], cwd=directory, env=env, stdout=log, stderr=log)
            try:
                for _ in range(100):
                    if process.poll() is not None:
                        raise AssertionError((Path(directory) / 'server.log').read_text())
                    try:
                        request(base + '/')
                        break
                    except OSError:
                        time.sleep(.05)
                else:
                    raise AssertionError('Server did not start')
                yield base, Path(directory)
            finally:
                process.terminate()
                process.wait(timeout=10)


def check():
    with server() as (base, directory):
        cases = [
            ('text', '原文 Copy ` ${value} <script> &', (), '/p/', ''),
            ('image', '', [('图片.png', PNG)], '/file/', '?preview=true'),
            ('file', '', [('sample.bin', b'\x00\x01\xff')], '/file/', ''),
            ('mixed', '图片说明', [('photo.png', PNG)], '/p/', ''),
            ('multiple', '', [('one.bin', b'one'), ('two.bin', b'two')], '/p/', ''),
            ('url', 'https://example.com/path?q=1', (), '/u/', ''),
        ]
        ids = {}
        for kind, content, files, prefix, suffix in cases:
            slug = upload(base, content, files)
            ids[kind] = slug
            status, headers, _ = request(f'{base}/{slug}')
            assert status == 302, (kind, status)
            assert headers['Location'] == f'{base}{prefix}{slug}{suffix}', (kind, headers)
            assert headers['Cache-Control'] == 'no-store'
            for alias in ('upload', 'p'):
                status, _, body = request(f'{base}/{alias}/{slug}')
                assert status == 200 and b'id="share-panel"' in body
                assert html.escape(f'{base}/{slug}').encode() in body
                assert b'<svg' in body, 'QR must be inline'
            if files:
                status, headers, body = request(f'{base}/file/{slug}{suffix}')
                assert status == 200 and body == files[0][1]
                assert headers['Content-Disposition'].startswith('inline' if suffix else 'attachment')
                status, _, part = request(f'{base}/file/{slug}', headers={'Range': 'bytes=0-1'})
                assert status == 206 and part == files[0][1][:2]
            if content and kind != 'url':
                assert request(f'{base}/raw/{slug}')[2].decode() == content
        status, headers, _ = request(f'{base}/u/{ids["url"]}')
        assert status == 302 and headers['Location'] == 'https://example.com/path?q=1'
        # A short redirect does not consume a read; the destination page counts it once.
        slug = ids['text']
        before = next(p['read_count'] for p in json.loads((directory / 'pasta_data/database.json').read_text()) if p['content'].startswith('原文'))
        request(f'{base}/{slug}')
        after = next(p['read_count'] for p in json.loads((directory / 'pasta_data/database.json').read_text()) if p['content'].startswith('原文'))
        assert before == after
        assert request(base + '/not-an-upload')[0] == 404
        assert request(base + '/static/theme.css')[0] == 200
        assert request(base + '/static/theme.js')[0] == 200
        assert request(base + '/guide')[0] == 200
        assert request(base + '/list')[0] == 200
        assert request(base + '/incorrect')[0] == 200
        assert request(base + '/admin')[0] == 302
        assert request(base + '/auth_admin')[0] == 200
        assert request(base + '/upload')[0] != 404, 'POST-only upload route remains reserved'
        if '--browser' in sys.argv:
            subprocess.run(['node', str(ROOT / 'tests/share_flow.cjs'), base, json.dumps(ids)], check=True)
        # Expired and removed entries must not resolve to files through the short route.
        request(f'{base}/remove/{ids["image"]}')
        assert request(f'{base}/{ids["image"]}')[0] == 404
        print('PASS: short routes, inline QR, file bytes/ranges, old routes, read counts and removed uploads')
    # Short domain and root IDs work independently of the upload/list login scope.
    with server(MICROBIN_SHORT_PATH='https://s.example', MICROBIN_BASIC_AUTH_USERNAME='owner', MICROBIN_BASIC_AUTH_PASSWORD='test') as (base, _):
        auth = 'Basic ' + base64.b64encode(b'owner:test').decode()
        slug = upload(base, 'short domain', auth=auth)
        assert request(base + '/list')[0] == 401
        assert request(base + '/incorrect')[0] == 401
        assert request(base + '/upload', b'')[0] == 401
        status, headers, _ = request(f'{base}/{slug}')
        assert status == 302 and headers['Location'] == f'{base}/p/{slug}'
        assert f'value="https://s.example/{slug}"'.encode() in request(f'{base}/p/{slug}')[2]
        print('PASS: separate short domain and protected route precedence')
    with server(MICROBIN_HASH_IDS='false', MICROBIN_PUBLIC_PATH='') as (base, _):
        slug = upload(base, 'animal identifier')
        status, headers, _ = request(f'{base}/{slug}')
        assert status == 302 and headers['Location'] == f'/p/{slug}'
        assert request(base + '/does-not-exist')[0] == 404
        print('PASS: animal IDs and relative URLs without a configured public domain')


if __name__ == '__main__':
    check()
