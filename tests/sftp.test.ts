import assert from 'node:assert/strict';
import test from 'node:test';
import {
  formatBytes,
  formatDuration,
  formatSpeed,
  parentPath,
  parseSftpInput,
  pathSegments,
  progressPercent,
} from '../frontend/src/lib/sftp.ts';

test('parseSftpInput parses sftp:// URIs with user, port and path', () => {
  assert.deepEqual(parseSftpInput('sftp://alice.1234@node1.example.com:2022/world/region'), {
    host: 'node1.example.com',
    port: 2022,
    username: 'alice.1234',
    path: '/world/region',
  });
});

test('parseSftpInput parses shorthand user@host:port and host:port', () => {
  assert.deepEqual(parseSftpInput('root@198.51.100.10:2222'), {
    host: '198.51.100.10',
    port: 2222,
    username: 'root',
  });
  assert.deepEqual(parseSftpInput('sftp.example.com:2022'), {
    host: 'sftp.example.com',
    port: 2022,
    username: undefined,
  });
  assert.equal(parseSftpInput('sftp.example.com'), null);
});

test('parentPath and pathSegments walk POSIX paths cleanly', () => {
  assert.equal(parentPath('/'), '/');
  assert.equal(parentPath('/world'), '/');
  assert.equal(parentPath('/world/region/'), '/world');

  assert.deepEqual(pathSegments('/srv/mc/plugins'), [
    { label: '/', path: '/' },
    { label: 'srv', path: '/srv' },
    { label: 'mc', path: '/srv/mc' },
    { label: 'plugins', path: '/srv/mc/plugins' },
  ]);
});

test('formatBytes, formatSpeed, formatDuration, and progressPercent format values', () => {
  assert.equal(formatBytes(0), '0 B');
  assert.equal(formatBytes(1536), '1.5 KiB');
  assert.equal(formatBytes(2 * 1024 * 1024 * 1024), '2.0 GiB');
  assert.equal(formatSpeed(0), '—');
  assert.equal(formatSpeed(5 * 1024 * 1024), '5.0 MiB/s');
  assert.equal(formatDuration(45), '45s');
  assert.equal(formatDuration(125), '2m 5s');
  assert.equal(formatDuration(3660), '1h 1m');
  assert.equal(progressPercent(50, 200, 1, 4), 25);
  assert.equal(progressPercent(0, 0, 3, 6), 50);
});
