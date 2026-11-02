#!/usr/bin/env python3
"""SQLite restart regression: python3 tests/sqlite_persistence.py."""
import contextlib
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time

from share_flow import BINARY, PNG, request, start_chunk, upload


@contextlib.contextmanager
def running(directory, base, port):
    env = {key: value for key, value in os.environ.items() if not key.startswith('MICROBIN_')}
    env.update({
        'MICROBIN_BIND': '127.0.0.1', 'MICROBIN_PORT': str(port),
        'MICROBIN_DATA_DIR': str(directory / 'store'), 'MICROBIN_JSON_DB': 'false',
        'MICROBIN_PUBLIC_PATH': base, 'MICROBIN_HASH_IDS': 'true',
        'MICROBIN_EDITABLE': 'true', 'MICROBIN_ETERNAL_PASTA': 'true',
        'MICROBIN_GC_DAYS': '0', 'MICROBIN_QR': 'true',
        'MICROBIN_DISABLE_TELEMETRY': 'true', 'MICROBIN_DISABLE_UPDATE_CHECKING': 'true',
        'MICROBIN_ADMIN_USERNAME': '', 'MICROBIN_ADMIN_PASSWORD': '',
    })
    with open(directory / 'server.log', 'ab') as log:
        process = subprocess.Popen([str(BINARY)], cwd=directory, env=env, stdout=log, stderr=log)
        try:
            for _ in range(100):
                if process.poll() is not None:
                    raise AssertionError((directory / 'server.log').read_text())
                try:
                    assert request(base + '/')[0] == 200
                    break
                except OSError:
                    time.sleep(.05)
            else:
                raise AssertionError('SQLite server did not start')
            yield
        finally:
            # Simulate abrupt container termination, without a graceful save hook.
            process.kill()
            process.wait(timeout=10)


def rows(directory):
    with sqlite3.connect(directory / 'store/database.sqlite') as conn:
        assert conn.execute('PRAGMA integrity_check').fetchone() == ('ok',)
        return conn.execute('SELECT * FROM pasta ORDER BY id').fetchall()


def check():
    with tempfile.TemporaryDirectory(prefix='microbin-sqlite-') as scratch:
        directory = Path(scratch)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        with running(directory, base, port):
            url = upload(base, 'https://example.org/sqlite-restart')
            image = upload(base, files=[('图片.png', PNG)])
            multiple = upload(base, '多文件说明', [('first.bin', b'first'), ('second.bin', b'second')])
            token = start_chunk(base, 'empty.bin', 0)
            empty = upload(base, tokens=[token])
            text = upload(base, '可编辑文本', extra=[('editable', 'on')])
            saved = rows(directory)
            assert len(saved) == 5
        with running(directory, base, port):
            assert rows(directory) == saved, 'Startup must not change persisted fields'
            for slug, target in [(url, '/u/'), (image, '/file/'), (multiple, '/p/'), (empty, '/file/')]:
                status, headers, _ = request(f'{base}/{slug}')
                assert status == 302 and headers['Location'].startswith(base + target), (slug, headers)
            assert request(f'{base}/u/{url}')[1]['Location'] == 'https://example.org/sqlite-restart'
            assert request(f'{base}/file/{image}?preview=true')[2] == PNG
            assert request(f'{base}/file/{empty}')[0] == 200
            page = request(f'{base}/upload/{multiple}')[2].decode()
            assert 'first.bin' in page and 'second.bin' in page
            assert request(f'{base}/raw/{multiple}')[2].decode() == '多文件说明'
            boundary = 'sqlite-edit'
            body = f'--{boundary}\r\nContent-Disposition: form-data; name="content"\r\n\r\n修改后的文本\r\n--{boundary}--\r\n'.encode()
            assert request(f'{base}/edit/{text}', body, {'Content-Type': f'multipart/form-data; boundary={boundary}'})[0] == 302
            assert request(f'{base}/raw/{text}')[2].decode() == '修改后的文本'
            assert request(f'{base}/remove/{image}')[0] == 302
            saved = rows(directory)
        with running(directory, base, port):
            assert rows(directory) == saved
            assert request(f'{base}/{image}')[0] == 404
            assert request(f'{base}/raw/{text}')[2].decode() == '修改后的文本'
            assert request(f'{base}/raw/{multiple}')[2].decode() == '多文件说明'
            assert request(f'{base}/u/{url}')[1]['Location'] == 'https://example.org/sqlite-restart'
            assert not (directory / 'pasta_data/database.json').exists()
    print('PASS: SQLite survives two abrupt restarts; URLs, images, attachments, empty files, edits and deletes remain correct')


if __name__ == '__main__':
    check()
