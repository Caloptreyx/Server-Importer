//! SFTP v3 client built directly on top of the panel's workspace `russh` crate.
//!
//! Speaking SFTP v3 (draft-ietf-secsh-filexfer-02) over a `russh` subsystem stream keeps the
//! extension free of extra crates.io dependencies while giving us full control over SSRF checks,
//! SSH host-key fingerprint pinning, keyboard-interactive password fallback, and pipelined reads.
use anyhow::{Context, anyhow, bail};
use russh::client::KeyboardInteractiveAuthResponse;
use serde::Serialize;
use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

const SFTP_VERSION: u32 = 3;
const MAX_PACKET_LEN: usize = 16 * 1024 * 1024;
const READ_CHUNK_SIZE: u32 = 32 * 1024;
const PIPELINE_DEPTH: usize = 8;
const MAX_SCAN_DEPTH: usize = 64;

const SSH_FXP_INIT: u8 = 1;
const SSH_FXP_VERSION: u8 = 2;
const SSH_FXP_OPEN: u8 = 3;
const SSH_FXP_CLOSE: u8 = 4;
const SSH_FXP_READ: u8 = 5;
const SSH_FXP_LSTAT: u8 = 7;
const SSH_FXP_OPENDIR: u8 = 11;
const SSH_FXP_READDIR: u8 = 12;
const SSH_FXP_REALPATH: u8 = 16;
const SSH_FXP_STAT: u8 = 17;
const SSH_FXP_STATUS: u8 = 101;
const SSH_FXP_HANDLE: u8 = 102;
const SSH_FXP_DATA: u8 = 103;
const SSH_FXP_NAME: u8 = 104;
const SSH_FXP_ATTRS: u8 = 105;

const SSH_FXF_READ: u32 = 0x0000_0001;

const SSH_FX_OK: u32 = 0;
const SSH_FX_EOF: u32 = 1;
const SSH_FX_NO_SUCH_FILE: u32 = 2;
const SSH_FX_PERMISSION_DENIED: u32 = 3;

const SSH_FILEXFER_ATTR_SIZE: u32 = 0x0000_0001;
const SSH_FILEXFER_ATTR_UIDGID: u32 = 0x0000_0002;
const SSH_FILEXFER_ATTR_PERMISSIONS: u32 = 0x0000_0004;
const SSH_FILEXFER_ATTR_ACMODTIME: u32 = 0x0000_0008;
const SSH_FILEXFER_ATTR_EXTENDED: u32 = 0x8000_0000;

const S_IFMT: u32 = 0o170_000;
const S_IFDIR: u32 = 0o040_000;
const S_IFREG: u32 = 0o100_000;
const S_IFLNK: u32 = 0o120_000;

/// Credentials kept strictly in memory for the lifetime of one request or import job.
pub enum SftpAuth {
    Password(String),
    PrivateKey {
        private_key: String,
        passphrase: Option<String>,
    },
}

impl SftpAuth {
    pub fn method_name(&self) -> &'static str {
        match self {
            Self::Password(_) => "password",
            Self::PrivateKey { .. } => "private_key",
        }
    }
}

pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SftpAuth,
    pub expected_fingerprint: Option<String>,
    pub timeout: Duration,
    pub allow_private_networks: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified_at: Option<i64>,
    pub permissions: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileAttrs {
    pub size: Option<u64>,
    pub permissions: Option<u32>,
    pub modified_at: Option<i64>,
}

impl FileAttrs {
    pub fn kind(&self, longname: Option<&str>) -> EntryKind {
        if let Some(mode) = self.permissions {
            match mode & S_IFMT {
                S_IFDIR => return EntryKind::Directory,
                S_IFREG => return EntryKind::File,
                S_IFLNK => return EntryKind::Symlink,
                0 => {}
                _ => return EntryKind::Other,
            }
        }
        if let Some(long) = longname {
            match long.as_bytes().first() {
                Some(b'd') => return EntryKind::Directory,
                Some(b'-') => return EntryKind::File,
                Some(b'l') => return EntryKind::Symlink,
                _ => {}
            }
        }
        EntryKind::Other
    }
}

#[derive(Clone, Debug)]
pub struct ScannedFile {
    pub remote_path: String,
    pub relative_path: String,
    pub size: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ScanResult {
    pub resolved_root: String,
    pub directories: Vec<String>,
    pub files: Vec<ScannedFile>,
    pub total_bytes: u64,
    pub symlink_count: u64,
    pub skipped_entries: u64,
    pub truncated: bool,
    pub truncation_reason: Option<String>,
}

struct FingerprintHandler {
    captured: Arc<Mutex<Option<String>>>,
    expected: Option<String>,
}

impl russh::client::Handler for FingerprintHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .public_key()
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string();
        if let Ok(mut guard) = self.captured.lock() {
            *guard = Some(fingerprint.clone());
        }
        if let Some(expected) = self.expected.as_deref() {
            let expected = expected.trim();
            if !expected.is_empty() && expected != fingerprint {
                tracing::warn!(
                    expected = expected,
                    actual = %fingerprint,
                    "Server Importer: remote SSH host key fingerprint mismatch"
                );
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Returns true if `ip` belongs to a loopback, private, link-local, multicast, unspecified,
/// CGNAT, or documentation/reserved range that must not be reached unless explicitly allowed.
pub fn is_private_or_special_ip(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => is_private_or_special_v4(v4),
        IpAddr::V6(v6) => is_private_or_special_v6(v6),
    }
}

fn is_private_or_special_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        // Shared Address Space / CGNAT (100.64.0.0/10)
        || (a == 100 && (64..=127).contains(&b))
        // IETF Protocol Assignments (192.0.0.0/24)
        || (a == 192 && b == 0 && c == 0)
        // Benchmarking (198.18.0.0/15)
        || (a == 198 && (b == 18 || b == 19))
        // Reserved (240.0.0.0/4)
        || a >= 240
}

fn is_private_or_special_v6(ip: Ipv6Addr) -> bool {
    let seg = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // Unique Local Addresses (fc00::/7)
        || (seg[0] & 0xfe00) == 0xfc00
        // Link-local (fe80::/10)
        || (seg[0] & 0xffc0) == 0xfe80
        // Deprecated site-local (fec0::/10)
        || (seg[0] & 0xffc0) == 0xfec0
        // Documentation (2001:db8::/32)
        || (seg[0] == 0x2001 && seg[1] == 0x0db8)
}

pub fn validate_host(host: &str) -> Result<String, anyhow::Error> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        bail!("Host is required");
    }
    if trimmed.len() > 255 {
        bail!("Host is too long");
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '/' || c == '@')
    {
        bail!("Host contains invalid characters");
    }
    Ok(trimmed.to_string())
}

pub async fn resolve_addresses(
    env: &Arc<shared::env::Env>,
    host: &str,
    port: u16,
    allow_private_networks: bool,
) -> Result<Vec<SocketAddr>, anyhow::Error> {
    let clean_host = validate_host(host)?;
    let unbracketed = shared::net::unbracket(&clean_host);

    if allow_private_networks {
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((unbracketed, port))
            .await
            .with_context(|| format!("Failed to resolve host {clean_host}"))?
            .collect();
        if addrs.is_empty() {
            bail!("Host {clean_host} did not resolve to any IP address");
        }
        return Ok(addrs);
    }

    let addrs = shared::net::resolve_allowed_addresses(env, unbracketed, port, "server-importer")
        .await
        .map_err(|err| anyhow!("Host {clean_host} is not allowed or could not be resolved: {err}"))?;

    let filtered: Vec<SocketAddr> = addrs
        .into_iter()
        .filter(|addr| !is_private_or_special_ip(addr.ip()))
        .collect();

    if filtered.is_empty() {
        bail!(
            "Connections to private, loopback, or internal IP addresses are disabled by the panel administrator"
        );
    }

    Ok(filtered)
}

pub fn normalize_remote_path(path: &str) -> Result<String, anyhow::Error> {
    let trimmed = path.trim();
    if trimmed.contains('\0') {
        bail!("Remote path contains an invalid NUL byte");
    }
    if trimmed.is_empty() || trimmed == "." {
        return Ok(".".to_string());
    }
    let normalized = trimmed.replace('\\', "/");
    if normalized == "/" {
        return Ok("/".to_string());
    }
    Ok(normalized.trim_end_matches('/').to_string())
}

pub fn normalize_destination_path(path: &str) -> Result<String, anyhow::Error> {
    let trimmed = path.trim();
    if trimmed.contains('\0') {
        bail!("Destination path contains an invalid NUL byte");
    }
    let normalized = trimmed.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for segment in normalized.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    bail!("Destination path cannot escape the server root");
                }
            }
            seg => parts.push(seg),
        }
    }
    if parts.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

pub fn join_posix(base: &str, child: &str) -> String {
    let child = child.trim_start_matches('/');
    if base.is_empty() || base == "." {
        return child.to_string();
    }
    if base == "/" {
        return format!("/{child}");
    }
    format!("{}/{}", base.trim_end_matches('/'), child)
}

/// An active SSH + SFTP v3 connection. Dropping it closes the underlying SSH session.
pub struct SftpSession {
    pub fingerprint: String,
    pub client: SftpClient<russh::ChannelStream<russh::client::Msg>>,
    _session: russh::client::Handle<FingerprintHandler>,
}

impl SftpSession {
    pub async fn connect(
        env: &Arc<shared::env::Env>,
        params: ConnectParams,
    ) -> Result<Self, anyhow::Error> {
        if params.username.trim().is_empty() {
            bail!("Username is required");
        }
        if params.port == 0 {
            bail!("Port must be between 1 and 65535");
        }

        let addresses = resolve_addresses(
            env,
            &params.host,
            params.port,
            params.allow_private_networks,
        )
        .await?;

        let captured = Arc::new(Mutex::new(None));
        let handler = FingerprintHandler {
            captured: Arc::clone(&captured),
            expected: params.expected_fingerprint.clone(),
        };

        let config = Arc::new(russh::client::Config {
            keepalive_interval: Some(Duration::from_secs(20)),
            keepalive_max: 3,
            ..Default::default()
        });

        let mut session = tokio::time::timeout(
            params.timeout,
            russh::client::connect(config, addresses.as_slice(), handler),
        )
        .await
        .map_err(|_| {
            anyhow!(
                "Timed out connecting to {}:{} after {}s",
                params.host,
                params.port,
                params.timeout.as_secs()
            )
        })?
        .map_err(|err| {
            let msg = err.to_string();
            if msg.contains("UnknownKey") || msg.contains("host key") {
                anyhow!(
                    "Remote SSH host key does not match the verified fingerprint. Test the connection again to inspect the new key."
                )
            } else {
                anyhow!("SSH connection failed: {err}")
            }
        })?;

        let fingerprint = captured
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .unwrap_or_default();

        authenticate_session(&mut session, &params.username, &params.auth, params.timeout).await?;

        let channel = tokio::time::timeout(params.timeout, session.channel_open_session())
            .await
            .map_err(|_| anyhow!("Timed out opening SSH channel"))?
            .context("Failed to open SSH session channel")?;

        tokio::time::timeout(params.timeout, channel.request_subsystem(true, "sftp"))
            .await
            .map_err(|_| anyhow!("Timed out starting SFTP subsystem"))?
            .context("Remote host rejected the SFTP subsystem request")?;

        let client = tokio::time::timeout(
            params.timeout,
            SftpClient::init(channel.into_stream()),
        )
        .await
        .map_err(|_| anyhow!("Timed out during SFTP handshake"))??;

        Ok(Self {
            fingerprint,
            client,
            _session: session,
        })
    }
}

async fn authenticate_session(
    session: &mut russh::client::Handle<FingerprintHandler>,
    username: &str,
    auth: &SftpAuth,
    timeout: Duration,
) -> Result<(), anyhow::Error> {
    match auth {
        SftpAuth::Password(password) => {
            let res = tokio::time::timeout(
                timeout,
                session.authenticate_password(username, password.as_str()),
            )
            .await
            .map_err(|_| anyhow!("Timed out authenticating with password"))?
            .context("SSH password authentication error")?;

            if res.success() {
                return Ok(());
            }

            // Many OpenSSH / PAM configurations disable plain `password` auth in favor of
            // `keyboard-interactive`. Try that automatically before failing.
            let mut kbd = tokio::time::timeout(
                timeout,
                session.authenticate_keyboard_interactive_start(username, None),
            )
            .await
            .map_err(|_| anyhow!("Timed out during keyboard-interactive authentication"))?
            .context("SSH keyboard-interactive authentication error")?;

            for _ in 0..4 {
                match kbd {
                    KeyboardInteractiveAuthResponse::Success => return Ok(()),
                    KeyboardInteractiveAuthResponse::Failure { .. } => {
                        bail!("Authentication failed: invalid username or password");
                    }
                    KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                        let responses = prompts.iter().map(|_| password.clone()).collect();
                        kbd = tokio::time::timeout(
                            timeout,
                            session.authenticate_keyboard_interactive_respond(responses),
                        )
                        .await
                        .map_err(|_| anyhow!("Timed out responding to authentication prompt"))?
                        .context("SSH keyboard-interactive response error")?;
                    }
                }
            }

            bail!("Authentication failed: invalid username or password");
        }
        SftpAuth::PrivateKey {
            private_key,
            passphrase,
        } => {
            let key = russh::keys::PrivateKey::from_openssh(private_key.trim())
                .map_err(|err| anyhow!("Invalid SSH private key: {err}"))?;
            let key = if key.is_encrypted() {
                let pass = passphrase
                    .as_deref()
                    .filter(|p| !p.is_empty())
                    .ok_or_else(|| {
                        anyhow!("This SSH private key is encrypted; enter its passphrase")
                    })?;
                key.decrypt(pass)
                    .map_err(|_| anyhow!("Failed to decrypt SSH private key: wrong passphrase"))?
            } else {
                key
            };

            let hash_alg = tokio::time::timeout(timeout, session.best_supported_rsa_hash())
                .await
                .map_err(|_| anyhow!("Timed out negotiating SSH key algorithm"))?
                .context("Failed to negotiate SSH key algorithm")?
                .flatten();

            let res = tokio::time::timeout(
                timeout,
                session.authenticate_publickey(
                    username,
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                ),
            )
            .await
            .map_err(|_| anyhow!("Timed out authenticating with SSH private key"))?
            .context("SSH private key authentication error")?;

            if !res.success() {
                bail!("Authentication failed: SSH private key was rejected by the remote host");
            }
            Ok(())
        }
    }
}

/// Low-level SFTP v3 client operating over any async byte stream.
pub struct SftpClient<S> {
    stream: S,
    next_id: u32,
}

impl<S: AsyncRead + AsyncWrite + Unpin> SftpClient<S> {
    pub async fn init(mut stream: S) -> Result<Self, anyhow::Error> {
        let mut init_pkt = Vec::with_capacity(9);
        init_pkt.extend_from_slice(&5u32.to_be_bytes());
        init_pkt.push(SSH_FXP_INIT);
        init_pkt.extend_from_slice(&SFTP_VERSION.to_be_bytes());
        stream
            .write_all(&init_pkt)
            .await
            .context("Failed to send SFTP INIT")?;
        stream.flush().await.context("Failed to flush SFTP INIT")?;

        let (pkt_type, payload) = read_packet(&mut stream).await?;
        if pkt_type != SSH_FXP_VERSION {
            bail!("Unexpected SFTP handshake packet type {pkt_type}");
        }
        let mut reader = PacketReader::new(&payload);
        let version = reader.read_u32()?;
        if version < 2 {
            bail!("Unsupported SFTP protocol version {version}");
        }

        Ok(Self { stream, next_id: 1 })
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    async fn send_packet(&mut self, pkt_type: u8, payload: &[u8]) -> Result<(), anyhow::Error> {
        let total_len = payload
            .len()
            .checked_add(1)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| anyhow!("SFTP packet too large"))?;
        self.stream.write_all(&total_len.to_be_bytes()).await?;
        self.stream.write_all(&[pkt_type]).await?;
        self.stream.write_all(payload).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn recv_for(&mut self, expected_id: u32) -> Result<(u8, Vec<u8>), anyhow::Error> {
        let (pkt_type, payload) = read_packet(&mut self.stream).await?;
        let mut reader = PacketReader::new(&payload);
        let id = reader.read_u32()?;
        if id != expected_id {
            bail!("SFTP response ID mismatch (expected {expected_id}, got {id})");
        }
        Ok((pkt_type, payload[4..].to_vec()))
    }

    pub async fn realpath(&mut self, path: &str) -> Result<String, anyhow::Error> {
        let id = self.alloc_id();
        let mut buf = Vec::with_capacity(8 + path.len());
        write_u32(&mut buf, id);
        write_str(&mut buf, path);
        self.send_packet(SSH_FXP_REALPATH, &buf).await?;

        let (pkt_type, body) = self.recv_for(id).await?;
        match pkt_type {
            SSH_FXP_NAME => {
                let mut r = PacketReader::new(&body);
                let count = r.read_u32()?;
                if count == 0 {
                    bail!("SFTP REALPATH returned no path for {path}");
                }
                let filename = r.read_string()?;
                Ok(filename)
            }
            SSH_FXP_STATUS => Err(parse_status_err(&body, &format!("resolve path {path}"))),
            other => bail!("Unexpected SFTP response {other} to REALPATH"),
        }
    }

    pub async fn stat(&mut self, path: &str, follow_links: bool) -> Result<FileAttrs, anyhow::Error> {
        let id = self.alloc_id();
        let mut buf = Vec::with_capacity(8 + path.len());
        write_u32(&mut buf, id);
        write_str(&mut buf, path);
        let req = if follow_links {
            SSH_FXP_STAT
        } else {
            SSH_FXP_LSTAT
        };
        self.send_packet(req, &buf).await?;

        let (pkt_type, body) = self.recv_for(id).await?;
        match pkt_type {
            SSH_FXP_ATTRS => {
                let mut r = PacketReader::new(&body);
                parse_attrs(&mut r)
            }
            SSH_FXP_STATUS => Err(parse_status_err(&body, &format!("stat {path}"))),
            other => bail!("Unexpected SFTP response {other} to STAT"),
        }
    }

    async fn opendir(&mut self, path: &str) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.alloc_id();
        let mut buf = Vec::with_capacity(8 + path.len());
        write_u32(&mut buf, id);
        write_str(&mut buf, path);
        self.send_packet(SSH_FXP_OPENDIR, &buf).await?;

        let (pkt_type, body) = self.recv_for(id).await?;
        match pkt_type {
            SSH_FXP_HANDLE => {
                let mut r = PacketReader::new(&body);
                r.read_bytes()
            }
            SSH_FXP_STATUS => Err(parse_status_err(&body, &format!("open directory {path}"))),
            other => bail!("Unexpected SFTP response {other} to OPENDIR"),
        }
    }

    async fn readdir(
        &mut self,
        handle: &[u8],
    ) -> Result<Option<Vec<(String, String, FileAttrs)>>, anyhow::Error> {
        let id = self.alloc_id();
        let mut buf = Vec::with_capacity(8 + handle.len());
        write_u32(&mut buf, id);
        write_bytes(&mut buf, handle);
        self.send_packet(SSH_FXP_READDIR, &buf).await?;

        let (pkt_type, body) = self.recv_for(id).await?;
        match pkt_type {
            SSH_FXP_NAME => {
                let mut r = PacketReader::new(&body);
                let count = r.read_u32()? as usize;
                let mut out = Vec::with_capacity(count.min(4096));
                for _ in 0..count {
                    let filename = r.read_string()?;
                    let longname = r.read_string()?;
                    let attrs = parse_attrs(&mut r)?;
                    out.push((filename, longname, attrs));
                }
                Ok(Some(out))
            }
            SSH_FXP_STATUS => {
                let (code, message) = parse_status(&body)?;
                if code == SSH_FX_EOF {
                    Ok(None)
                } else {
                    Err(status_error(code, &message, "read directory"))
                }
            }
            other => bail!("Unexpected SFTP response {other} to READDIR"),
        }
    }

    async fn open_read(&mut self, path: &str) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.alloc_id();
        let mut buf = Vec::with_capacity(16 + path.len());
        write_u32(&mut buf, id);
        write_str(&mut buf, path);
        write_u32(&mut buf, SSH_FXF_READ);
        write_u32(&mut buf, 0); // empty ATTRS flags
        self.send_packet(SSH_FXP_OPEN, &buf).await?;

        let (pkt_type, body) = self.recv_for(id).await?;
        match pkt_type {
            SSH_FXP_HANDLE => {
                let mut r = PacketReader::new(&body);
                r.read_bytes()
            }
            SSH_FXP_STATUS => Err(parse_status_err(&body, &format!("open file {path}"))),
            other => bail!("Unexpected SFTP response {other} to OPEN"),
        }
    }

    async fn close(&mut self, handle: &[u8]) -> Result<(), anyhow::Error> {
        let id = self.alloc_id();
        let mut buf = Vec::with_capacity(8 + handle.len());
        write_u32(&mut buf, id);
        write_bytes(&mut buf, handle);
        self.send_packet(SSH_FXP_CLOSE, &buf).await?;

        let (pkt_type, body) = self.recv_for(id).await?;
        match pkt_type {
            SSH_FXP_STATUS => {
                let (code, message) = parse_status(&body)?;
                if code == SSH_FX_OK {
                    Ok(())
                } else {
                    Err(status_error(code, &message, "close handle"))
                }
            }
            other => bail!("Unexpected SFTP response {other} to CLOSE"),
        }
    }

    /// Resolves `path` and lists its entries sorted directories-first, then alphabetically.
    pub async fn list_dir(&mut self, path: &str) -> Result<(String, Vec<RemoteEntry>), anyhow::Error> {
        let clean = normalize_remote_path(path)?;
        let resolved = self.realpath(&clean).await.unwrap_or_else(|_| {
            if clean == "." {
                "/".to_string()
            } else {
                clean.clone()
            }
        });

        let handle = self.opendir(&resolved).await?;
        let mut entries = Vec::new();
        let read_result: Result<(), anyhow::Error> = async {
            while let Some(batch) = self.readdir(&handle).await? {
                for (name, longname, attrs) in batch {
                    if name == "." || name == ".." || name.is_empty() {
                        continue;
                    }
                    let kind = attrs.kind(Some(&longname));
                    let entry_path = join_posix(&resolved, &name);
                    entries.push(RemoteEntry {
                        name,
                        path: entry_path,
                        kind,
                        size: attrs.size.unwrap_or(0),
                        modified_at: attrs.modified_at,
                        permissions: attrs.permissions,
                    });
                }
            }
            Ok(())
        }
        .await;

        let _ = self.close(&handle).await;
        read_result?;

        entries.sort_by(|a, b| {
            let a_dir = a.kind == EntryKind::Directory;
            let b_dir = b.kind == EntryKind::Directory;
            b_dir
                .cmp(&a_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok((resolved, entries))
    }

    /// Recursively scans a remote file or directory tree without following symlinks.
    pub async fn scan_tree(
        &mut self,
        path: &str,
        max_files: Option<u64>,
        max_bytes: Option<u64>,
        cancel: &CancellationToken,
        stop_on_limit: bool,
    ) -> Result<ScanResult, anyhow::Error> {
        let clean = normalize_remote_path(path)?;
        let resolved = self.realpath(&clean).await.unwrap_or_else(|_| {
            if clean == "." {
                "/".to_string()
            } else {
                clean.clone()
            }
        });

        let root_attrs = self.stat(&resolved, true).await?;
        let root_kind = root_attrs.kind(None);

        let mut result = ScanResult {
            resolved_root: resolved.clone(),
            ..Default::default()
        };

        if root_kind == EntryKind::File {
            let size = root_attrs.size.unwrap_or(0);
            if let Some(limit) = max_bytes
                && size > limit
            {
                if stop_on_limit {
                    result.truncated = true;
                    result.truncation_reason = Some(format!(
                        "Source exceeds the maximum allowed import size ({} MiB)",
                        limit / (1024 * 1024)
                    ));
                    result.total_bytes = size;
                    return Ok(result);
                }
                bail!(
                    "Source exceeds the maximum allowed import size ({} MiB)",
                    limit / (1024 * 1024)
                );
            }
            let file_name = resolved
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("file")
                .to_string();
            result.files.push(ScannedFile {
                remote_path: resolved,
                relative_path: file_name,
                size,
            });
            result.total_bytes = size;
            return Ok(result);
        }

        if root_kind != EntryKind::Directory {
            bail!("Source path {resolved} is neither a regular file nor a directory");
        }

        let mut queue: VecDeque<(String, String, usize)> = VecDeque::new();
        queue.push_back((resolved, String::new(), 0));

        while let Some((remote_dir, rel_dir, depth)) = queue.pop_front() {
            if cancel.is_cancelled() {
                bail!("Import cancelled");
            }
            if depth > MAX_SCAN_DEPTH {
                result.skipped_entries += 1;
                continue;
            }

            let handle = match self.opendir(&remote_dir).await {
                Ok(h) => h,
                Err(err) => {
                    if rel_dir.is_empty() {
                        return Err(err);
                    }
                    tracing::debug!(path = %remote_dir, error = %err, "Server Importer: skipping unreadable remote directory");
                    result.skipped_entries += 1;
                    continue;
                }
            };

            let mut dir_entries = Vec::new();
            let read_res: Result<(), anyhow::Error> = async {
                while let Some(batch) = self.readdir(&handle).await? {
                    dir_entries.extend(batch);
                }
                Ok(())
            }
            .await;
            let _ = self.close(&handle).await;
            if let Err(err) = read_res {
                if rel_dir.is_empty() {
                    return Err(err);
                }
                result.skipped_entries += 1;
                continue;
            }

            dir_entries.sort_by(|a, b| a.0.cmp(&b.0));

            for (name, longname, attrs) in dir_entries {
                if cancel.is_cancelled() {
                    bail!("Import cancelled");
                }
                if name == "." || name == ".." || name.is_empty() || name.contains('/') {
                    continue;
                }

                let child_remote = join_posix(&remote_dir, &name);
                let child_rel = if rel_dir.is_empty() {
                    name.clone()
                } else {
                    format!("{rel_dir}/{name}")
                };

                let mut kind = attrs.kind(Some(&longname));
                let mut size = attrs.size.unwrap_or(0);
                if kind == EntryKind::Other
                    && let Ok(lstat) = self.stat(&child_remote, false).await
                {
                    kind = lstat.kind(None);
                    size = lstat.size.unwrap_or(size);
                }

                match kind {
                    EntryKind::Directory => {
                        result.directories.push(child_rel.clone());
                        queue.push_back((child_remote, child_rel, depth + 1));
                    }
                    EntryKind::File => {
                        if let Some(limit) = max_files
                            && (result.files.len() as u64) >= limit
                        {
                            let msg = format!(
                                "Source exceeds the maximum allowed file count ({limit} files)"
                            );
                            if stop_on_limit {
                                result.truncated = true;
                                result.truncation_reason = Some(msg);
                                return Ok(result);
                            }
                            bail!(msg);
                        }
                        let new_total = result.total_bytes.saturating_add(size);
                        if let Some(limit) = max_bytes
                            && new_total > limit
                        {
                            let msg = format!(
                                "Source exceeds the maximum allowed import size ({} MiB)",
                                limit / (1024 * 1024)
                            );
                            if stop_on_limit {
                                result.truncated = true;
                                result.truncation_reason = Some(msg);
                                return Ok(result);
                            }
                            bail!(msg);
                        }
                        result.total_bytes = new_total;
                        result.files.push(ScannedFile {
                            remote_path: child_remote,
                            relative_path: child_rel,
                            size,
                        });
                    }
                    EntryKind::Symlink => {
                        result.symlink_count += 1;
                    }
                    EntryKind::Other => {
                        result.skipped_entries += 1;
                    }
                }
            }
        }

        Ok(result)
    }

    /// Streams a remote file to `sink` using pipelined SFTP `READ` requests while invoking
    /// `on_chunk(bytes)` as data flows. Safely adapts to servers that return short reads.
    pub async fn stream_file_to<W, F>(
        &mut self,
        remote_path: &str,
        expected_size: u64,
        sink: &mut W,
        cancel: &CancellationToken,
        mut on_chunk: F,
    ) -> Result<u64, anyhow::Error>
    where
        W: AsyncWrite + Unpin,
        F: FnMut(u64),
    {
        let handle = self.open_read(remote_path).await?;
        let transfer = self
            .stream_handle_to(&handle, expected_size, sink, cancel, &mut on_chunk)
            .await;
        // Always attempt to close the remote file handle even if streaming failed or was cancelled.
        let _ = self.close(&handle).await;
        transfer
    }

    async fn stream_handle_to<W, F>(
        &mut self,
        handle: &[u8],
        expected_size: u64,
        sink: &mut W,
        cancel: &CancellationToken,
        on_chunk: &mut F,
    ) -> Result<u64, anyhow::Error>
    where
        W: AsyncWrite + Unpin,
        F: FnMut(u64),
    {
        let mut chunk_size = READ_CHUNK_SIZE;
        let mut written_offset: u64 = 0;
        let mut next_request_offset: u64 = 0;
        let mut inflight: VecDeque<(u32, u64, u32)> = VecDeque::with_capacity(PIPELINE_DEPTH);

        loop {
            if cancel.is_cancelled() {
                self.drain_inflight(&mut inflight).await;
                bail!("Import cancelled");
            }

            // Determine how many pipelined reads to keep in flight. For tiny/unknown-size files
            // we still issue at least one read so we reliably detect EOF.
            let desired_depth = if expected_size == 0 {
                1
            } else {
                let remaining = expected_size.saturating_sub(next_request_offset);
                let needed = remaining.div_ceil(u64::from(chunk_size)) as usize;
                needed.clamp(1, PIPELINE_DEPTH)
            };

            let mut queued_any = false;
            while inflight.len() < desired_depth
                && (next_request_offset < expected_size || inflight.is_empty())
            {
                let req_len = if expected_size > next_request_offset {
                    let rem = expected_size - next_request_offset;
                    rem.min(u64::from(chunk_size)) as u32
                } else {
                    chunk_size
                };
                let id = self.alloc_id();
                let mut pkt = Vec::with_capacity(20 + handle.len());
                write_u32(&mut pkt, id);
                write_bytes(&mut pkt, handle);
                write_u64(&mut pkt, next_request_offset);
                write_u32(&mut pkt, req_len);
                let total_len = (pkt.len() + 1) as u32;
                self.stream.write_all(&total_len.to_be_bytes()).await?;
                self.stream.write_all(&[SSH_FXP_READ]).await?;
                self.stream.write_all(&pkt).await?;
                inflight.push_back((id, next_request_offset, req_len));
                next_request_offset = next_request_offset.saturating_add(u64::from(req_len));
                queued_any = true;
            }
            if queued_any {
                self.stream.flush().await?;
            }

            let Some((expected_id, offset, requested_len)) = inflight.pop_front() else {
                break;
            };

            let (pkt_type, body) = match self.recv_for(expected_id).await {
                Ok(v) => v,
                Err(err) => {
                    self.drain_inflight(&mut inflight).await;
                    return Err(err);
                }
            };

            match pkt_type {
                SSH_FXP_DATA => {
                    let mut r = PacketReader::new(&body);
                    let data = r.read_bytes()?;
                    if data.is_empty() {
                        self.drain_inflight(&mut inflight).await;
                        break;
                    }
                    sink.write_all(&data).await?;
                    let len = data.len() as u64;
                    written_offset = offset.saturating_add(len);
                    on_chunk(len);

                    // If the server returned fewer bytes than requested, any already-pipelined
                    // requests start past the actual next byte. Drain them, lower chunk_size to
                    // match the server's max read window, and resume cleanly from `written_offset`.
                    if (data.len() as u32) < requested_len {
                        self.drain_inflight(&mut inflight).await;
                        next_request_offset = written_offset;
                        chunk_size = (data.len() as u32).clamp(4096, READ_CHUNK_SIZE);
                    }
                }
                SSH_FXP_STATUS => {
                    let (code, message) = parse_status(&body)?;
                    self.drain_inflight(&mut inflight).await;
                    if code == SSH_FX_EOF {
                        break;
                    }
                    return Err(status_error(code, &message, "read file"));
                }
                other => {
                    self.drain_inflight(&mut inflight).await;
                    bail!("Unexpected SFTP response {other} to READ");
                }
            }
        }

        sink.flush().await?;
        Ok(written_offset)
    }

    async fn drain_inflight(&mut self, inflight: &mut VecDeque<(u32, u64, u32)>) {
        while let Some((id, _, _)) = inflight.pop_front() {
            if self.recv_for(id).await.is_err() {
                inflight.clear();
                break;
            }
        }
    }
}

async fn read_packet<S: AsyncRead + Unpin>(stream: &mut S) -> Result<(u8, Vec<u8>), anyhow::Error> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .context("Connection closed while reading SFTP packet length")?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_PACKET_LEN {
        bail!("Invalid SFTP packet length {len}");
    }
    let mut pkt_type = [0u8; 1];
    stream.read_exact(&mut pkt_type).await?;
    let mut payload = vec![0u8; len - 1];
    if !payload.is_empty() {
        stream.read_exact(&mut payload).await?;
    }
    Ok((pkt_type[0], payload))
}

struct PacketReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> PacketReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn read_u32(&mut self) -> Result<u32, anyhow::Error> {
        if self.remaining() < 4 {
            bail!("Truncated SFTP packet (expected u32)");
        }
        let v = u32::from_be_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    fn read_u64(&mut self) -> Result<u64, anyhow::Error> {
        if self.remaining() < 8 {
            bail!("Truncated SFTP packet (expected u64)");
        }
        let v = u64::from_be_bytes(self.buf[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }

    fn read_bytes(&mut self) -> Result<Vec<u8>, anyhow::Error> {
        let len = self.read_u32()? as usize;
        if self.remaining() < len {
            bail!("Truncated SFTP packet (expected {len} bytes)");
        }
        let out = self.buf[self.pos..self.pos + len].to_vec();
        self.pos += len;
        Ok(out)
    }

    fn read_string(&mut self) -> Result<String, anyhow::Error> {
        let bytes = self.read_bytes()?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

fn write_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_be_bytes());
}

fn write_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    write_u32(buf, data.len() as u32);
    buf.extend_from_slice(data);
}

fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_bytes(buf, s.as_bytes());
}

fn parse_attrs(r: &mut PacketReader<'_>) -> Result<FileAttrs, anyhow::Error> {
    let flags = r.read_u32()?;
    let mut attrs = FileAttrs::default();
    if flags & SSH_FILEXFER_ATTR_SIZE != 0 {
        attrs.size = Some(r.read_u64()?);
    }
    if flags & SSH_FILEXFER_ATTR_UIDGID != 0 {
        let _uid = r.read_u32()?;
        let _gid = r.read_u32()?;
    }
    if flags & SSH_FILEXFER_ATTR_PERMISSIONS != 0 {
        attrs.permissions = Some(r.read_u32()?);
    }
    if flags & SSH_FILEXFER_ATTR_ACMODTIME != 0 {
        let _atime = r.read_u32()?;
        let mtime = r.read_u32()?;
        attrs.modified_at = Some(i64::from(mtime));
    }
    if flags & SSH_FILEXFER_ATTR_EXTENDED != 0 {
        let count = r.read_u32()?;
        for _ in 0..count {
            let _k = r.read_bytes()?;
            let _v = r.read_bytes()?;
        }
    }
    Ok(attrs)
}

fn parse_status(body: &[u8]) -> Result<(u32, String), anyhow::Error> {
    let mut r = PacketReader::new(body);
    let code = r.read_u32()?;
    let message = if r.remaining() >= 4 {
        r.read_string().unwrap_or_default()
    } else {
        String::new()
    };
    Ok((code, message))
}

fn parse_status_err(body: &[u8], action: &str) -> anyhow::Error {
    match parse_status(body) {
        Ok((code, message)) => status_error(code, &message, action),
        Err(err) => err,
    }
}

fn status_error(code: u32, message: &str, action: &str) -> anyhow::Error {
    let reason = match code {
        SSH_FX_NO_SUCH_FILE => "not found",
        SSH_FX_PERMISSION_DENIED => "permission denied",
        _ if !message.is_empty() => message,
        _ => "remote SFTP error",
    };
    if !message.is_empty() && reason != message {
        anyhow!("Failed to {action}: {reason} ({message})")
    } else {
        anyhow!("Failed to {action}: {reason} (code {code})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn private_and_special_ips_are_blocked() {
        assert!(is_private_or_special_ip("127.0.0.1".parse().unwrap()));
        assert!(is_private_or_special_ip("10.0.0.5".parse().unwrap()));
        assert!(is_private_or_special_ip("172.16.1.1".parse().unwrap()));
        assert!(is_private_or_special_ip("192.168.1.10".parse().unwrap()));
        assert!(is_private_or_special_ip("169.254.169.254".parse().unwrap()));
        assert!(is_private_or_special_ip("100.64.0.1".parse().unwrap()));
        assert!(is_private_or_special_ip("::1".parse().unwrap()));
        assert!(is_private_or_special_ip("fc00::1".parse().unwrap()));
        assert!(is_private_or_special_ip("fe80::1".parse().unwrap()));
        assert!(is_private_or_special_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_private_or_special_ip("::ffff:10.1.2.3".parse().unwrap()));

        assert!(!is_private_or_special_ip("1.1.1.1".parse().unwrap()));
        assert!(!is_private_or_special_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_or_special_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn destination_paths_stay_inside_root() {
        assert_eq!(normalize_destination_path("").unwrap(), "/");
        assert_eq!(normalize_destination_path("/").unwrap(), "/");
        assert_eq!(normalize_destination_path("world/region/").unwrap(), "/world/region");
        assert_eq!(normalize_destination_path("/a/b/../c").unwrap(), "/a/c");
        assert!(normalize_destination_path("/../etc/passwd").is_err());
        assert!(normalize_destination_path("a/../../outside").is_err());
        assert!(normalize_destination_path("/bad\0path").is_err());
    }

    #[test]
    fn posix_join_handles_roots() {
        assert_eq!(join_posix("/", "plugins"), "/plugins");
        assert_eq!(join_posix("/srv/mc/", "world"), "/srv/mc/world");
        assert_eq!(join_posix(".", "server.properties"), "server.properties");
    }

    /// Tiny in-memory SFTP v3 server for testing real wire framing, directory listing,
    /// symlink skipping, and pipelined streaming (including forced short reads!).
    async fn run_mock_sftp(
        mut stream: tokio::io::DuplexStream,
        files: HashMap<String, Vec<u8>>,
        max_read_len: usize,
    ) {
        let Ok((pkt_type, _)) = read_packet(&mut stream).await else {
            return;
        };
        assert_eq!(pkt_type, SSH_FXP_INIT);
        let mut resp = Vec::new();
        write_u32(&mut resp, 5);
        resp.push(SSH_FXP_VERSION);
        write_u32(&mut resp, 3);
        let _ = stream.write_all(&resp).await;

        let mut dir_handles: HashMap<Vec<u8>, (String, bool)> = HashMap::new();
        let mut file_handles: HashMap<Vec<u8>, String> = HashMap::new();
        let mut handle_seq: u32 = 1;

        while let Ok((pkt_type, payload)) = read_packet(&mut stream).await {
            let mut r = PacketReader::new(&payload);
            let id = r.read_u32().unwrap();
            let mut body = Vec::new();
            write_u32(&mut body, id);

            let out_type = match pkt_type {
                SSH_FXP_REALPATH => {
                    let p = r.read_string().unwrap();
                    let resolved = if p == "." || p.is_empty() { "/".to_string() } else { p };
                    write_u32(&mut body, 1);
                    write_str(&mut body, &resolved);
                    write_str(&mut body, &resolved);
                    write_u32(&mut body, 0);
                    SSH_FXP_NAME
                }
                SSH_FXP_STAT | SSH_FXP_LSTAT => {
                    let p = r.read_string().unwrap();
                    if let Some(data) = files.get(&p) {
                        write_u32(
                            &mut body,
                            SSH_FILEXFER_ATTR_SIZE | SSH_FILEXFER_ATTR_PERMISSIONS,
                        );
                        write_u64(&mut body, data.len() as u64);
                        write_u32(&mut body, S_IFREG | 0o644);
                        SSH_FXP_ATTRS
                    } else if p == "/" || p == "/sub" {
                        write_u32(&mut body, SSH_FILEXFER_ATTR_PERMISSIONS);
                        write_u32(&mut body, S_IFDIR | 0o755);
                        SSH_FXP_ATTRS
                    } else {
                        write_u32(&mut body, SSH_FX_NO_SUCH_FILE);
                        write_str(&mut body, "not found");
                        write_str(&mut body, "");
                        SSH_FXP_STATUS
                    }
                }
                SSH_FXP_OPENDIR => {
                    let p = r.read_string().unwrap();
                    let h = handle_seq.to_be_bytes().to_vec();
                    handle_seq += 1;
                    dir_handles.insert(h.clone(), (p, false));
                    write_bytes(&mut body, &h);
                    SSH_FXP_HANDLE
                }
                SSH_FXP_READDIR => {
                    let h = r.read_bytes().unwrap();
                    let entry = dir_handles.get_mut(&h).unwrap();
                    if entry.1 {
                        write_u32(&mut body, SSH_FX_EOF);
                        write_str(&mut body, "eof");
                        write_str(&mut body, "");
                        SSH_FXP_STATUS
                    } else {
                        entry.1 = true;
                        let dir = entry.0.clone();
                        let mut items: Vec<(String, u32, u64)> = Vec::new();
                        if dir == "/" {
                            items.push(("sub".into(), S_IFDIR | 0o755, 0));
                            items.push(("link".into(), S_IFLNK | 0o777, 0));
                            if let Some(d) = files.get("/root.bin") {
                                items.push(("root.bin".into(), S_IFREG | 0o644, d.len() as u64));
                            }
                        } else if dir == "/sub" {
                            if let Some(d) = files.get("/sub/nested.txt") {
                                items.push(("nested.txt".into(), S_IFREG | 0o644, d.len() as u64));
                            }
                        }
                        write_u32(&mut body, items.len() as u32);
                        for (name, mode, size) in items {
                            write_str(&mut body, &name);
                            write_str(&mut body, &name);
                            write_u32(
                                &mut body,
                                SSH_FILEXFER_ATTR_SIZE | SSH_FILEXFER_ATTR_PERMISSIONS,
                            );
                            write_u64(&mut body, size);
                            write_u32(&mut body, mode);
                        }
                        SSH_FXP_NAME
                    }
                }
                SSH_FXP_OPEN => {
                    let p = r.read_string().unwrap();
                    let _flags = r.read_u32().unwrap();
                    let _attrs = parse_attrs(&mut r).unwrap();
                    let h = handle_seq.to_be_bytes().to_vec();
                    handle_seq += 1;
                    file_handles.insert(h.clone(), p);
                    write_bytes(&mut body, &h);
                    SSH_FXP_HANDLE
                }
                SSH_FXP_READ => {
                    let h = r.read_bytes().unwrap();
                    let offset = r.read_u64().unwrap() as usize;
                    let len = (r.read_u32().unwrap() as usize).min(max_read_len);
                    let path = file_handles.get(&h).unwrap();
                    let data = files.get(path).unwrap();
                    if offset >= data.len() {
                        write_u32(&mut body, SSH_FX_EOF);
                        write_str(&mut body, "eof");
                        write_str(&mut body, "");
                        SSH_FXP_STATUS
                    } else {
                        let end = (offset + len).min(data.len());
                        write_bytes(&mut body, &data[offset..end]);
                        SSH_FXP_DATA
                    }
                }
                SSH_FXP_CLOSE => {
                    let h = r.read_bytes().unwrap();
                    dir_handles.remove(&h);
                    file_handles.remove(&h);
                    write_u32(&mut body, SSH_FX_OK);
                    write_str(&mut body, "ok");
                    write_str(&mut body, "");
                    SSH_FXP_STATUS
                }
                _ => {
                    write_u32(&mut body, 8);
                    write_str(&mut body, "unsupported");
                    write_str(&mut body, "");
                    SSH_FXP_STATUS
                }
            };

            let total_len = (body.len() + 1) as u32;
            let _ = stream.write_all(&total_len.to_be_bytes()).await;
            let _ = stream.write_all(&[out_type]).await;
            let _ = stream.write_all(&body).await;
            let _ = stream.flush().await;
        }
    }

    #[tokio::test]
    async fn sftp_client_lists_scans_and_streams_with_short_reads() {
        let large_payload: Vec<u8> = (0..(200 * 1024)).map(|i| (i % 251) as u8).collect();
        let nested_payload = b"hello calagopus".to_vec();

        let mut files = HashMap::new();
        files.insert("/root.bin".into(), large_payload.clone());
        files.insert("/sub/nested.txt".into(), nested_payload.clone());

        let (client_stream, server_stream) = tokio::io::duplex(256 * 1024);
        // Force 11 KiB server max read length (< 32 KiB client chunk) to exercise short-read recovery!
        tokio::spawn(run_mock_sftp(server_stream, files, 11 * 1024));

        let mut client = SftpClient::init(client_stream).await.unwrap();
        let (resolved, entries) = client.list_dir("/").await.unwrap();
        assert_eq!(resolved, "/");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "sub");
        assert_eq!(entries[0].kind, EntryKind::Directory);

        let cancel = CancellationToken::new();
        let scan = client.scan_tree("/", None, None, &cancel, false).await.unwrap();
        assert_eq!(scan.directories, vec!["sub".to_string()]);
        assert_eq!(scan.files.len(), 2);
        assert_eq!(scan.symlink_count, 1);
        assert_eq!(
            scan.total_bytes,
            (large_payload.len() + nested_payload.len()) as u64
        );

        let mut out = Vec::new();
        let mut reported = 0u64;
        let written = client
            .stream_file_to(
                "/root.bin",
                large_payload.len() as u64,
                &mut out,
                &cancel,
                |n| reported += n,
            )
            .await
            .unwrap();
        assert_eq!(written, large_payload.len() as u64);
        assert_eq!(reported, large_payload.len() as u64);
        assert_eq!(out, large_payload);
    }
}
