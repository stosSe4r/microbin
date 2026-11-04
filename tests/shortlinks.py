#!/usr/bin/env python3
"""Finite format exhaustion, concurrent allocation and batch management regressions."""
import concurrent.futures
import json
import re
import socket
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path
from share_flow import ROOT, PNG, request, server, upload
from sqlite_persistence import running


def availability(base, format='custom', custom='[xy]'):
    return request(base + '/upload/options/availability', json.dumps({'format': format, 'custom': custom}).encode(), {'Content-Type': 'application/json'})


def create(base, pattern, content='test'):
    boundary = 'finite-format-test'
    fields = [('short_link_format', 'custom'), ('short_link_custom', pattern), ('content', content), ('privacy', 'public')]
    body = ''.join(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n' for name, value in fields)
    return request(base + '/upload', (body + f'--{boundary}--\r\n').encode(), {'Content-Type': f'multipart/form-data; boundary={boundary}'})


def batch(base, slugs):
    return request(base + '/list/remove', json.dumps({'slugs': slugs}).encode(), {'Content-Type': 'application/json'})


def check():
    with server(MICROBIN_THREADS='4') as (base, directory):
        for preset, regex in [('lower5', '[a-hjkmnp-z]{5}'), ('lower6', '[a-hjkmnp-z]{6}'), ('digits6', '[0-9]{6}'), ('mixed6', '[a-zA-Z2-9]{6}')]:
            slug = upload(base, 'preset ' + preset, files=[('图片.png', PNG)], extra=[('short_link_format', preset)])
            assert re.fullmatch(regex, slug), (preset, slug)
            assert request(f'{base}/file/{slug}')[2] == PNG
            assert request(f'{base}/raw/{slug}')[2] == ('preset ' + preset).encode()
        assert availability(base, 'custom', '[a-z]*')[0] == 400
        assert create(base, '../bad')[0] == 400
        assert create(base, 'list')[0] == 409, 'Built-in routes cannot become share names'
        assert json.loads(availability(base, 'custom', 'list')[2])['available'] is False
        initial = json.loads(availability(base)[2])
        assert initial['capacity'] == initial['remaining'] == '2'
        # Twelve simultaneous requests for exactly four names: four creations, eight clear conflicts.
        with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
            results = list(pool.map(lambda n: create(base, 'tiny-[ab]{2}', str(n)), range(12)))
        assert [r[0] for r in results].count(302) == 4, [r[0] for r in results]
        assert [r[0] for r in results].count(409) == 8
        names = [r[1]['Location'].rsplit('/', 1)[-1] for r in results if r[0] == 302]
        assert len(set(names)) == 4
        assert json.loads(availability(base, 'custom', 'tiny-[ab]{2}')[2])['remaining'] == '0'
        assert batch(base, names[:1])[0] == 200
        assert create(base, 'tiny-[ab]{2}')[0] == 302, 'The one remaining slot must always be found'
        image = upload(base, files=[('batch.png', PNG)])
        readonly = upload(base, 'protected', privacy='readonly')
        private = upload(base, 'unlisted', privacy='unlisted')
        assert batch(base, [image, readonly])[0] == 403
        assert batch(base, [image, private])[0] == 403
        assert batch(base, [image, 'missing-share'])[0] == 409
        assert request(f'{base}/file/{image}')[2] == PNG, 'A rejected batch must not change other shares'
        assert batch(base, [image, image])[0] == 200
        assert request(f'{base}/{image}')[0] == 404
        assert not (directory / 'pasta_data/attachments' / image).exists()
        if '--browser' in sys.argv:
            # Browser tests get a small list spanning all upload types.
            subprocess.run(['node', str(ROOT / 'tests/shortlinks.cjs'), base], check=True)
    with server(MICROBIN_EDITABLE='false') as (base, _):
        immutable = upload(base, 'immutable')
        assert batch(base, [immutable])[0] == 403
        if '--browser' in sys.argv:
            subprocess.run(['node', str(ROOT / 'tests/shortlinks.cjs'), base, '--immutable'], check=True)
    with tempfile.TemporaryDirectory(prefix='microbin-slugs-') as scratch:
        directory = Path(scratch)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        with running(directory, base, port, MICROBIN_SHORT_LINK_FORMAT='legacy'):
            old = upload(base, files=[('old.png', PNG)])
        # Reproduce the actual previous SQLite schema, with no short-link column.
        with sqlite3.connect(directory / 'store/database.sqlite') as conn:
            schema = conn.execute("SELECT sql FROM sqlite_master WHERE type='table' AND name='pasta'").fetchone()[0]
            conn.execute(schema.replace('CREATE TABLE pasta', 'CREATE TABLE pasta_legacy').replace('    slug TEXT,\n', ''))
            columns = ','.join(row[1] for row in conn.execute('PRAGMA table_info(pasta)') if row[1] != 'slug')
            conn.execute(f'INSERT INTO pasta_legacy ({columns}) SELECT {columns} FROM pasta')
            conn.execute('DROP TABLE pasta')
            conn.execute('ALTER TABLE pasta_legacy RENAME TO pasta')
        with running(directory, base, port):
            assert request(f'{base}/file/{old}')[2] == PNG
            new = upload(base, 'new format', extra=[('short_link_format', 'custom'), ('short_link_custom', 'new-[2-3]{2}')])
            expired = upload(base, 'expired', extra=[('short_link_format', 'custom'), ('short_link_custom', 'expired-slot')])
        with sqlite3.connect(directory / 'store/database.sqlite') as conn:
            conn.execute('UPDATE pasta SET expiration=1 WHERE slug=?', (expired,))
        with running(directory, base, port, MICROBIN_SHORT_LINK_FORMAT='digits6'):
            assert request(f'{base}/file/{old}')[2] == PNG
            assert request(f'{base}/raw/{new}')[2] == b'new format'
            assert json.loads(availability(base, 'custom', 'expired-slot')[2])['remaining'] == '1'
            assert request(f'{base}/{expired}')[0] == 404
            assert re.fullmatch('[0-9]{6}', upload(base, 'changed default'))
            assert batch(base, [new])[0] == 200
            with sqlite3.connect(directory / 'store/database.sqlite') as conn:
                assert conn.execute('PRAGMA integrity_check').fetchone() == ('ok',)
        with running(directory, base, port):
            assert request(f'{base}/{new}')[0] == 404, 'Batch deletion persists across restart'
            assert request(f'{base}/file/{old}')[2] == PNG
    with tempfile.TemporaryDirectory(prefix='microbin-json-slugs-') as scratch:
        directory = Path(scratch)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        with running(directory, base, port, MICROBIN_JSON_DB='true', MICROBIN_DATA_DIR='pasta_data'):
            old = upload(base, files=[('json.png', PNG)])
            expired = upload(base, 'expired', extra=[('short_link_format', 'custom'), ('short_link_custom', 'expired-slot')])
        db = directory / 'pasta_data/database.json'
        rows = json.loads(db.read_text())
        for row in rows:
            if row['slug'] == expired: row['expiration'] = 1
        db.write_text(json.dumps(rows))
        with running(directory, base, port, MICROBIN_JSON_DB='true', MICROBIN_DATA_DIR='pasta_data', MICROBIN_SHORT_LINK_FORMAT='digits6'):
            assert json.loads(availability(base, 'custom', 'expired-slot')[2])['remaining'] == '1'
            assert request(f'{base}/file/{old}')[2] == PNG
            assert request(f'{base}/{expired}')[0] == 404
            assert re.fullmatch('[0-9]{6}', upload(base, 'changed JSON default'))
    print('PASS: presets, finite patterns, capacity exhaustion, concurrent allocation, one free slot, reserved routes, batch permissions/files and SQLite migration/restarts')


if __name__ == '__main__':
    check()
