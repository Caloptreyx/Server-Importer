# Changelog

## 0.1.0

- Initial release of Server Importer (`dev.caloptreyx.serverimporter`).
- SFTP v3 client over Calagopus's workspace `russh` with password, keyboard-interactive, and OpenSSH private key authentication.
- Encrypted per-user saved SFTP logins (using the panel's `APP_ENCRYPTION_KEY`) with save/select/edit/delete support and an admin toggle (`allow_saved_logins`).
- Test connection, SSH host-key SHA256 fingerprint verification/pinning, remote folder browser with search filtering, and recursive size preview.
- Direct SFTP-to-Wings streaming imports with live progress, speed/ETA, cancellation, and persisted per-server import history.
- Admin settings for concurrency, size/file limits, connection timeout, saved logins, and private-network SSRF protection.
