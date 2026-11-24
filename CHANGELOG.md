# Changelog

## 0.1.0

- Initial release of Server Importer (`dev.caloptreyx.serverimporter`).
- SFTP v3 client over Calagopus's workspace `russh` with password, keyboard-interactive, and OpenSSH private key authentication.
- Test connection, SSH host-key SHA256 fingerprint verification/pinning, remote folder browser, and recursive size preview.
- Direct SFTP-to-Wings streaming imports with live progress, speed/ETA, cancellation, and persisted per-server import history.
- Admin settings for concurrency, size/file limits, connection timeout, and private-network SSRF protection.
