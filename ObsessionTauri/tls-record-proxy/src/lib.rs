//! Local, fixed-target TCP adapter. TLS remains end-to-end between client and server.
//! Only the complete first ClientHello is reconstructed and split into TLS records.
use std::{
    collections::BTreeSet,
    future::Future,
    io,
    net::{IpAddr, SocketAddr},
    ops::Range,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
    time::{interval, timeout},
};

const MAX_RECORD_BYTES: usize = 16_384;
const MAX_HELLO_BYTES: usize = 65_536;
const MAX_INPUT_RECORDS: usize = 256;
static CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: SocketAddr,
    pub target: SocketAddr,
    pub server_name: String,
    pub fragment_bytes: usize,
    pub max_connections: usize,
    pub hello_timeout: Duration,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub connection_lifetime: Duration,
    /// Static status and counts only. No packet, TLS, URL, or application payloads.
    pub log_events: bool,
}

impl Config {
    pub fn new(listen: SocketAddr, target: SocketAddr, server_name: String) -> Self {
        Self {
            listen,
            target,
            server_name,
            fragment_bytes: 512,
            max_connections: 16,
            hello_timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(60),
            connection_lifetime: Duration::from_secs(300),
            log_events: false,
        }
    }

    pub fn validate(&self) -> io::Result<()> {
        let exact_loopback = match self.listen.ip() {
            IpAddr::V4(ip) => ip.octets() == [127, 0, 0, 1],
            IpAddr::V6(ip) => ip == std::net::Ipv6Addr::LOCALHOST,
        };
        if !exact_loopback {
            return invalid("listener must be exactly 127.0.0.1 or ::1");
        }
        if self.target.port() == 0
            || self.target.ip().is_unspecified()
            || self.target.ip().is_multicast()
            || self.target == self.listen
            || self.target.ip() == IpAddr::V4(std::net::Ipv4Addr::BROADCAST)
        {
            return invalid("target must be a fixed unicast IP:port distinct from listener");
        }
        if !valid_dns_name(&self.server_name) {
            return invalid("expected SNI must be an ASCII DNS name");
        }
        if !(32..=MAX_RECORD_BYTES).contains(&self.fragment_bytes) {
            return invalid("fragment size must be between 32 and 16384 bytes");
        }
        if !(1..=128).contains(&self.max_connections) {
            return invalid("connection cap must be between 1 and 128");
        }
        for duration in [
            self.hello_timeout,
            self.connect_timeout,
            self.idle_timeout,
            self.connection_lifetime,
        ] {
            if duration.is_zero() || duration > Duration::from_secs(86_400) {
                return invalid("timeouts must be positive and no greater than 24 hours");
            }
        }
        Ok(())
    }
}

fn valid_dns_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.is_ascii()
        && name.contains('.')
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        && name.parse::<IpAddr>().is_err()
}

fn invalid<T>(message: &'static str) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidData, message))
}

fn bounded_slice(bytes: &[u8], start: usize, length: usize) -> io::Result<&[u8]> {
    bytes
        .get(
            start
                ..start
                    .checked_add(length)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "length overflow"))?,
        )
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated ClientHello"))
}

fn u16_at(bytes: &[u8], offset: usize) -> io::Result<usize> {
    let value = bounded_slice(bytes, offset, 2)?;
    Ok(usize::from(u16::from_be_bytes([value[0], value[1]])))
}

/// Return the hostname's byte range in the unmodified handshake message.
fn sni_range(handshake: &[u8], expected: &str) -> io::Result<Range<usize>> {
    if handshake.len() < 4 || handshake[0] != 1 {
        return invalid("first handshake must be ClientHello");
    }
    let declared =
        ((handshake[1] as usize) << 16) | ((handshake[2] as usize) << 8) | handshake[3] as usize;
    if declared + 4 != handshake.len() {
        return invalid("ClientHello length mismatch");
    }
    if bounded_slice(handshake, 4, 2)? != [3, 3] {
        return invalid("unsupported ClientHello legacy version");
    }
    bounded_slice(handshake, 6, 32)?;
    let session_len = *bounded_slice(handshake, 38, 1)?.first().unwrap() as usize;
    if session_len > 32 {
        return invalid("invalid session ID length");
    }
    let mut position = 39 + session_len;
    bounded_slice(handshake, 39, session_len)?;
    let ciphers = u16_at(handshake, position)?;
    if ciphers < 2 || ciphers % 2 != 0 {
        return invalid("invalid cipher suite list");
    }
    position += 2;
    bounded_slice(handshake, position, ciphers)?;
    position += ciphers;
    let compression = bounded_slice(handshake, position, 1)?[0] as usize;
    position += 1;
    let compression_methods = bounded_slice(handshake, position, compression)?;
    if compression_methods != [0] {
        return invalid("unsupported TLS compression methods");
    }
    position += compression;
    let extension_length = u16_at(handshake, position)?;
    position += 2;
    if position + extension_length != handshake.len() {
        return invalid("extension list length mismatch");
    }
    let mut extension_types = BTreeSet::new();
    let mut hostname = None;
    while position < handshake.len() {
        let extension_type = u16_at(handshake, position)?;
        let length = u16_at(handshake, position + 2)?;
        position += 4;
        let extension = bounded_slice(handshake, position, length)?;
        if !extension_types.insert(extension_type) {
            return invalid("duplicate TLS extension");
        }
        if extension_type == 0 {
            if extension.len() < 2 || u16_at(extension, 0)? + 2 != extension.len() {
                return invalid("invalid server name list");
            }
            let mut name_position = 2;
            let mut name_types = BTreeSet::new();
            while name_position < extension.len() {
                let name_type = bounded_slice(extension, name_position, 1)?[0];
                let name_length = u16_at(extension, name_position + 1)?;
                name_position += 3;
                let name = bounded_slice(extension, name_position, name_length)?;
                if !name_types.insert(name_type) {
                    return invalid("duplicate server name type");
                }
                if name_type == 0 {
                    let name_str = std::str::from_utf8(name)
                        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-ASCII SNI"))?;
                    if !valid_dns_name(name_str) || !name_str.eq_ignore_ascii_case(expected) {
                        return invalid("SNI does not match the fixed target name");
                    }
                    hostname =
                        Some(position + name_position..position + name_position + name_length);
                }
                name_position += name_length;
            }
        }
        position += length;
    }
    hostname.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "ClientHello has no expected SNI",
        )
    })
}

#[derive(Debug)]
pub struct PreparedHello {
    pub wire: Vec<u8>,
    pub input_records: usize,
    pub output_records: usize,
}

fn push_record(wire: &mut Vec<u8>, version: [u8; 2], payload: &[u8]) {
    wire.extend_from_slice(&[22, version[0], version[1]]);
    wire.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    wire.extend_from_slice(payload);
}

/// Read only through the TLS record containing the end of ClientHello.
/// A later message sharing that record is retained without changing its bytes.
pub async fn prepare_client_hello<R: AsyncRead + Unpin>(
    reader: &mut R,
    expected: &str,
    fragment_bytes: usize,
) -> io::Result<PreparedHello> {
    if !valid_dns_name(expected) || !(32..=MAX_RECORD_BYTES).contains(&fragment_bytes) {
        return invalid("invalid record fragmentation parameters");
    }
    let mut handshake = Vec::with_capacity(4096);
    let mut declared = None;
    let mut first_version = [0, 0];
    let mut record_count = 0;
    loop {
        if record_count >= MAX_INPUT_RECORDS {
            return invalid("too many ClientHello input records");
        }
        let mut header = [0u8; 5];
        reader.read_exact(&mut header).await?;
        if header[0] != 22 || header[1] != 3 || ![1, 2, 3].contains(&header[2]) {
            return invalid("expected a plaintext TLS handshake record");
        }
        if record_count == 0 {
            first_version = [header[1], header[2]];
        }
        let length = u16::from_be_bytes([header[3], header[4]]) as usize;
        if length == 0 || length > MAX_RECORD_BYTES {
            return invalid("TLS record size out of bounds");
        }
        // At most one complete record of trailing handshake data is buffered.
        if handshake.len() + length > MAX_HELLO_BYTES + MAX_RECORD_BYTES {
            return invalid("ClientHello buffer limit exceeded");
        }
        let start = handshake.len();
        handshake.resize(start + length, 0);
        reader.read_exact(&mut handshake[start..]).await?;
        record_count += 1;
        if handshake[0] != 1 {
            return invalid("first handshake must be ClientHello");
        }
        if declared.is_none() && handshake.len() >= 4 {
            let message_len = 4
                + ((handshake[1] as usize) << 16)
                + ((handshake[2] as usize) << 8)
                + handshake[3] as usize;
            if !(45..=MAX_HELLO_BYTES).contains(&message_len) {
                return invalid("ClientHello length out of bounds");
            }
            declared = Some(message_len);
        }
        if let Some(message_len) = declared {
            if handshake.len() >= message_len {
                let hello = &handshake[..message_len];
                let sni = sni_range(hello, expected)?;
                // Deliberately split the visible hostname across TLS records.
                let semantic_split = sni.start + (sni.len() / 2);
                let mut wire = Vec::with_capacity(
                    handshake.len() + (handshake.len() / fragment_bytes + 3) * 5,
                );
                let mut output_records = 0;
                let mut position = 0;
                while position < hello.len() {
                    let mut end = (position + fragment_bytes).min(hello.len());
                    if position < semantic_split && semantic_split < end {
                        end = semantic_split;
                    }
                    push_record(&mut wire, first_version, &hello[position..end]);
                    output_records += 1;
                    position = end;
                }
                let tail = &handshake[message_len..];
                if !tail.is_empty() {
                    push_record(&mut wire, [header[1], header[2]], tail);
                    output_records += 1;
                }
                return Ok(PreparedHello {
                    wire,
                    input_records: record_count,
                    output_records,
                });
            }
        }
    }
}

async fn copy_direction<R, W>(
    mut reader: R,
    mut writer: W,
    activity: Arc<Mutex<Instant>>,
) -> io::Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = [0u8; 8192];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            writer.shutdown().await?;
            return Ok(total);
        }
        *activity.lock().unwrap() = Instant::now();
        writer.write_all(&buffer[..read]).await?;
        *activity.lock().unwrap() = Instant::now();
        total += read as u64;
    }
}

async fn relay(client: &mut TcpStream, upstream: &mut TcpStream, cfg: &Config) -> io::Result<()> {
    let activity = Arc::new(Mutex::new(Instant::now()));
    let (client_read, client_write) = client.split();
    let (upstream_read, upstream_write) = upstream.split();
    let transfer = async {
        tokio::try_join!(
            copy_direction(client_read, upstream_write, activity.clone()),
            copy_direction(upstream_read, client_write, activity.clone())
        )?;
        Ok(())
    };
    let idle = async {
        let mut ticker = interval(
            (cfg.idle_timeout / 4)
                .max(Duration::from_millis(10))
                .min(Duration::from_secs(1)),
        );
        loop {
            ticker.tick().await;
            if activity.lock().unwrap().elapsed() >= cfg.idle_timeout {
                break;
            }
        }
    };
    tokio::select! {
        result = transfer => result,
        _ = idle => Err(io::Error::new(io::ErrorKind::TimedOut, "connection idle limit exceeded")),
    }
}

/// Invalid or incomplete hello never causes a connection to the upstream target.
pub async fn handle_connection(mut client: TcpStream, cfg: &Config) -> io::Result<()> {
    cfg.validate()?;
    let id = CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let result = timeout(cfg.connection_lifetime, async {
        client.set_nodelay(true)?;
        let prepared = timeout(
            cfg.hello_timeout,
            prepare_client_hello(&mut client, &cfg.server_name, cfg.fragment_bytes),
        )
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ClientHello deadline exceeded"))??;
        if cfg.log_events {
            eprintln!(
                "connection {id}: hello records {} -> {}; prepared {} bytes",
                prepared.input_records,
                prepared.output_records,
                prepared.wire.len()
            );
        }
        let mut upstream = timeout(cfg.connect_timeout, TcpStream::connect(cfg.target))
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "target connection deadline exceeded",
                )
            })??;
        upstream.set_nodelay(true)?;
        timeout(cfg.hello_timeout, upstream.write_all(&prepared.wire))
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "first flight send deadline exceeded",
                )
            })??;
        relay(&mut client, &mut upstream, cfg).await
    })
    .await
    .unwrap_or_else(|_| {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "connection lifetime exceeded",
        ))
    });
    if cfg.log_events {
        match &result {
            Ok(()) => eprintln!(
                "connection {id}: finished; elapsed {} ms",
                started.elapsed().as_millis()
            ),
            Err(error) => eprintln!(
                "connection {id}: failed {:?}; elapsed {} ms",
                error.kind(),
                started.elapsed().as_millis()
            ),
        }
    }
    result
}

pub async fn bind(cfg: &Config) -> io::Result<TcpListener> {
    cfg.validate()?;
    TcpListener::bind(cfg.listen).await
}

/// Resource bounded accept loop. Shutdown closes the listener and all active relays.
/// Connection failures are intentionally not logged with handshake/client payloads.
pub async fn run<F: Future<Output = ()>>(
    listener: TcpListener,
    cfg: Config,
    shutdown: F,
) -> io::Result<()> {
    cfg.validate()?;
    let actual = listener.local_addr()?;
    if actual.ip() != cfg.listen.ip()
        || (cfg.listen.port() != 0 && actual.port() != cfg.listen.port())
        || cfg.target == actual
    {
        return invalid("bound listener does not match configuration");
    }
    let permits = Arc::new(Semaphore::new(cfg.max_connections));
    let mut tasks = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        // Completed tasks release permits immediately, but their results must also
        // be drained so an accept storm cannot accumulate JoinSet entries.
        while tasks.try_join_next().is_some() {}
        tokio::select! {
            _ = &mut shutdown => break,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
            accepted = listener.accept() => {
                let (client, _) = accepted?;
                if tasks.len() >= cfg.max_connections {
                    if cfg.log_events { eprintln!("connection rejected: capacity limit"); }
                    drop(client); continue;
                }
                match permits.clone().try_acquire_owned() {
                    Ok(permit) => {
                        let config = cfg.clone();
                        tasks.spawn(async move {
                            let _permit = permit;
                            handle_connection(client, &config).await
                        });
                    },
                    Err(_) => {
                        if cfg.log_events { eprintln!("connection rejected: capacity limit"); }
                        drop(client);
                    },
                }
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}
