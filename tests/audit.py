#!/usr/bin/env python3
"""Request/IP audit persistence and retention: python3 tests/audit.py."""
import concurrent.futures
import json
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
from share_flow import PNG, request, upload, start_chunk, server
from sqlite_persistence import running


def logs(directory):
    with sqlite3.connect(directory / 'store/audit.sqlite') as db:
        db.row_factory = sqlite3.Row
        return [dict(row) for row in db.execute('SELECT * FROM operation_log ORDER BY id')]


def latest(directory, **values):
    return next(row for row in reversed(logs(directory)) if all(row[key] == value for key, value in values.items()))


def batch(base, slugs):
    return request(base + '/list/remove', json.dumps({'slugs': slugs}).encode(), {'Content-Type': 'application/json', 'X-Forwarded-For': '198.51.100.90, 192.0.2.1'})


def check():
    with tempfile.TemporaryDirectory(prefix='microbin-audit-') as scratch:
        directory = Path(scratch)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        with running(directory, base, port, MICROBIN_THREADS='4'):
            cases = [
                ({}, '127.0.0.1', 'peer'),
                ({'EO-Connecting-IP': '203.0.113.7', 'EO-LOG-UUID': 'edge-test-123'}, '203.0.113.7', 'eo-connecting-ip'),
                ({'X-Forwarded-For': '198.51.100.2, 203.0.113.7', 'EO-Connecting-IP': '203.0.113.7'}, '198.51.100.2', 'x-forwarded-for'),
                ({'X-Forwarded-For': '2001:db8::2, 203.0.113.7'}, '2001:db8::2', 'x-forwarded-for'),
                ({'X-Forwarded-For': 'unknown, 198.51.100.4', 'EO-Connecting-IP': '203.0.113.8'}, '203.0.113.8', 'eo-connecting-ip'),
                ({'X-Forwarded-For': 'unknown', 'EO-Connecting-IP': 'not-an-ip', 'X-Real-IP': '[2001:db8::3]:443'}, '2001:db8::3', 'x-real-ip'),
                ({'X-Forwarded-For': 'unknown', 'EO-Connecting-IP': 'not-an-ip', 'X-Real-IP': 'unknown', 'Forwarded': 'for=198.51.100.9'}, '127.0.0.1', 'peer'),
            ]
            for headers, expected, source in cases:
                assert request(base + '/list?private-query=do-not-record', headers=headers)[0] == 200
                row = latest(directory, path='/list')
                assert (row['client_ip'], row['ip_source'], row['peer_ip']) == (expected, source, '127.0.0.1'), row
                assert row['edgeone_ip'] == headers.get('EO-Connecting-IP')
                assert row['forwarded_for'] == headers.get('X-Forwarded-For')
                assert row['edgeone_request_id'] == headers.get('EO-LOG-UUID')
                assert row['expires_at'] - row['created_at'] == 30 * 86400
            text = upload(base, 'body-do-not-record', extra=[('plain_key', 'password-do-not-record')])
            image = upload(base, files=[('audit.png', PNG)])
            protected = upload(base, 'readonly', privacy='readonly')
            for slug in [text, image, protected]:
                row = latest(directory, operation='create', share_slug=slug)
                assert row['expires_at'] is None and row['status'] == 302
            assert request(base + '/' + image)[0] == 302
            assert latest(directory, path='/' + image)['share_slug'] == image
            assert request(base + '/file/' + image, headers={'Range': 'bytes=0-1'})[0] == 206
            assert latest(directory, path='/file/' + image)['status'] == 206
            assert request(base + '/static/theme.js')[0] == 200
            assert latest(directory, path='/static/theme.js')['operation'] == 'access'
            boundary = 'audit-edit'
            body = f'--{boundary}\r\nContent-Disposition: form-data; name="content"\r\n\r\nchanged-do-not-record\r\n--{boundary}--\r\n'.encode()
            assert request(base + '/edit/' + text, body, {'Content-Type': f'multipart/form-data; boundary={boundary}'})[0] == 302
            assert latest(directory, operation='edit')['share_slug'] == text
            token = start_chunk(base, 'staged.bin', 3)
            assert request(f'{base}/upload/chunks/{token}/0', b'abc', method='PUT')[0] == 200
            assert request(f'{base}/upload/chunks/{token}/1', b'abc', method='PUT')[0] == 400
            assert latest(directory, operation='upload_chunk')['status'] == 400
            assert latest(directory, operation='upload_chunk')['share_slug'] is None
            assert request(f'{base}/upload/chunks/{token}', method='DELETE')[0] == 204
            assert latest(directory, operation='upload_cancel')['expires_at'] is not None
            assert batch(base, [text, protected])[0] == 403
            row = latest(directory, operation='batch_delete')
            assert row['status'] == 403 and json.loads(row['targets']) == [text, protected]
            assert request(base + '/remove/' + protected)[0] == 302
            assert latest(directory, operation='delete')['redirect_path'] == '/auth_remove_private/' + protected
            assert row['client_ip'] == '198.51.100.90' and row['expires_at'] is None
            assert batch(base, [text])[0] == 200
            assert json.loads(latest(directory, operation='batch_delete')['targets']) == [text]
            assert request(base + '/remove/' + image)[0] == 302
            assert latest(directory, operation='delete')['share_slug'] == image
            assert latest(directory, operation='delete')['redirect_path'] == '/list'
            assert request(base + '/list/remove', b'{broken', {'Content-Type': 'application/json'})[0] == 400
            assert latest(directory, operation='batch_delete')['status'] == 400
            result = subprocess.check_output(['curl', '--noproxy', '*', '--http2-prior-knowledge', '--max-time', '10', '-fsS',
                '-H', 'X-Forwarded-For: 2001:db8::9', '-o', '/dev/null', '-w', '%{http_version}', base + '/list'], text=True)
            assert result == '2' and latest(directory, path='/list')['client_ip'] == '2001:db8::9'
            # Each response waits for its committed audit row, including concurrent failures.
            with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
                statuses = list(pool.map(lambda n: request(f'{base}/static/audit-missing-{n}')[0], range(100)))
            assert statuses == [404] * 100
            rows = logs(directory)
            assert len([r for r in rows if r['path'].startswith('/static/audit-missing-')]) == 100
            assert all(r['duration_ms'] >= 0 for r in rows)
            dump = json.dumps(rows)
            for secret in ['body-do-not-record', 'password-do-not-record', 'changed-do-not-record', 'private-query']:
                assert secret not in dump
            saved = len(rows)
        # Inject expired/recent samples while stopped, then exercise automatic startup retention.
        with sqlite3.connect(directory / 'store/audit.sqlite') as db:
            now = int(time.time())
            for path, operation, expiry in [('/expired-access', 'access', now - 1), ('/expired-chunk', 'upload_chunk', now - 1),
                    ('/old-important', 'delete', None), ('/recent-access', 'access', now + 600)]:
                db.execute('INSERT INTO operation_log(created_at,operation,method,path,status,duration_ms,ip_source,expires_at) VALUES(1,?,?,?,?,0,?,?)',
                    (operation, 'GET', path, 200, 'peer', expiry))
            assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
            assert db.execute('PRAGMA auto_vacuum').fetchone() == (2,)
        with running(directory, base, port):
            rows = logs(directory)
            paths = {r['path'] for r in rows}
            assert '/expired-access' not in paths and '/expired-chunk' not in paths
            assert '/old-important' in paths and '/recent-access' in paths
            assert len(rows) >= saved + 2
            assert request(base + '/' + text)[0] == 404 and request(base + '/' + image)[0] == 404
            assert latest(directory, operation='create', share_slug=protected)['expires_at'] is None
    with server(MICROBIN_BASIC_AUTH_USERNAME='audit-owner', MICROBIN_BASIC_AUTH_PASSWORD='not-logged') as (base, directory):
        assert request(base + '/list')[0] == 401
        with sqlite3.connect(directory / 'pasta_data/audit.sqlite') as db:
            assert db.execute("SELECT status FROM operation_log WHERE path='/list' ORDER BY id DESC LIMIT 1").fetchone() == (401,)
    print('PASS: forwarding/peer IPs, IPv6/fallbacks, all HTTP routes/errors/auth, create/edit/delete/batch targets, H2C, 100 concurrent writes, no body secrets, crash persistence and automatic retention')


if __name__ == '__main__':
    check()
