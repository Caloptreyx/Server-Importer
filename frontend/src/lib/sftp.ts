export interface ParsedSftpInput {
  host: string;
  port?: number;
  username?: string;
  path?: string;
}

/**
 * Parses pasted SFTP/SSH connection strings such as:
 * - `sftp://user@example.com:2022/world`
 * - `ssh://user@1.2.3.4:22`
 * - `user@example.com:2022`
 * - `example.com:2022`
 * - `[2001:db8::1]:2022`
 *
 * Returns `null` when the string looks like a plain hostname without extra parts.
 */
export function parseSftpInput(raw: string): ParsedSftpInput | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;

  if (/^(sftp|ssh):\/\//i.test(trimmed)) {
    try {
      const normalized = trimmed.replace(/^(sftp|ssh):\/\//i, 'http://');
      const url = new URL(normalized);
      if (!url.hostname) return null;
      const result: ParsedSftpInput = {
        host: url.hostname,
      };
      if (url.port) {
        const parsedPort = Number.parseInt(url.port, 10);
        if (parsedPort >= 1 && parsedPort <= 65535) {
          result.port = parsedPort;
        }
      }
      if (url.username) {
        result.username = decodeURIComponent(url.username);
      }
      if (url.pathname && url.pathname !== '/') {
        result.path = decodeURIComponent(url.pathname);
      }
      return result;
    } catch {
      return null;
    }
  }

  let rest = trimmed;
  let username: string | undefined;
  const atIndex = rest.lastIndexOf('@');
  if (atIndex > 0) {
    username = rest.slice(0, atIndex);
    rest = rest.slice(atIndex + 1);
  }

  // Handle bracketed IPv6 like `[2001:db8::1]:2022`
  if (rest.startsWith('[')) {
    const closeBracket = rest.indexOf(']');
    if (closeBracket === -1) return null;
    const host = rest.slice(0, closeBracket + 1);
    const after = rest.slice(closeBracket + 1);
    if (after.startsWith(':')) {
      const port = Number.parseInt(after.slice(1), 10);
      if (port >= 1 && port <= 65535) {
        return { host, port, username };
      }
    }
    return username ? { host, username } : null;
  }

  // Single colon => host:port (avoid splitting bare IPv6 addresses with multiple colons)
  const firstColon = rest.indexOf(':');
  const lastColon = rest.lastIndexOf(':');
  if (firstColon > 0 && firstColon === lastColon) {
    const host = rest.slice(0, firstColon);
    const portStr = rest.slice(firstColon + 1);
    if (/^\d+$/.test(portStr)) {
      const port = Number.parseInt(portStr, 10);
      if (port >= 1 && port <= 65535) {
        return { host, port, username };
      }
    }
  }

  if (username && rest) {
    return { host: rest, username };
  }

  return null;
}

export function parentPath(path: string): string {
  const cleaned = path.replace(/\/+$/, '');
  if (!cleaned || cleaned === '/') return '/';
  const idx = cleaned.lastIndexOf('/');
  if (idx <= 0) return '/';
  return cleaned.slice(0, idx);
}

export interface BreadcrumbSegment {
  label: string;
  path: string;
}

export function pathSegments(path: string): BreadcrumbSegment[] {
  const trimmed = path.trim();
  if (!trimmed || trimmed === '/' || trimmed === '.') {
    return [{ label: '/', path: '/' }];
  }
  const parts = trimmed.split('/').filter(Boolean);
  const segments: BreadcrumbSegment[] = [{ label: '/', path: '/' }];
  let acc = '';
  for (const part of parts) {
    acc = `${acc}/${part}`;
    segments.push({ label: part, path: acc });
  }
  return segments;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let value = bytes;
  let idx = 0;
  while (value >= 1024 && idx < units.length - 1) {
    value /= 1024;
    idx += 1;
  }
  return idx === 0 ? `${Math.round(value)} B` : `${value.toFixed(value >= 100 ? 0 : 1)} ${units[idx]}`;
}

export function formatSpeed(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return '—';
  return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return '—';
  const secs = Math.floor(totalSeconds);
  if (secs < 60) return `${secs}s`;
  const hours = Math.floor(secs / 3600);
  const minutes = Math.floor((secs % 3600) / 60);
  const remSecs = secs % 60;
  if (hours > 0) {
    return remSecs > 0 ? `${hours}h ${minutes}m ${remSecs}s` : `${hours}h ${minutes}m`;
  }
  return remSecs > 0 ? `${minutes}m ${remSecs}s` : `${minutes}m`;
}

export function progressPercent(
  bytesTransferred: number,
  bytesTotal: number,
  filesTransferred: number,
  filesTotal: number,
): number {
  if (bytesTotal > 0) {
    return Math.min(100, Math.max(0, (bytesTransferred / bytesTotal) * 100));
  }
  if (filesTotal > 0) {
    return Math.min(100, Math.max(0, (filesTransferred / filesTotal) * 100));
  }
  return 0;
}
