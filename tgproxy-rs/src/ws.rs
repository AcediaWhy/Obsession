//! Минимальный RFC6455-клиент поверх tokio + rustls (порт RawWebSocket).
//!
//! Отличие от апстрима: TLS к DC верифицируется по webpki-корням с
//! проверкой SNI — вместо `CERT_NONE`.

use std::io::IoSlice;
use std::sync::Arc;

use base64::Engine;
use rand::RngCore;
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

pub const MAX_MESSAGE_LEN: usize = 16 * 1024 * 1024;
const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const WS_WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const TCP_KEEPALIVE_IDLE: std::time::Duration = std::time::Duration::from_secs(60);
const TCP_KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

const OP_CONT: u8 = 0x0;
const OP_BINARY: u8 = 0x2;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

#[derive(Debug)]
pub enum ConnectError {
    Timeout,
    Io(std::io::Error),
    Tls(std::io::Error),
    Handshake(WsHandshakeError),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Timeout => write!(f, "timed out"),
            ConnectError::Io(error) | ConnectError::Tls(error) => write!(f, "{error}"),
            ConnectError::Handshake(error) => write!(f, "{error}"),
        }
    }
}

#[derive(Debug)]
pub struct WsHandshakeError {
    pub status_code: u16,
    pub status_line: String,
    pub location: Option<String>,
}

impl std::fmt::Display for WsHandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status_code, self.status_line)?;
        if let Some(location) = &self.location {
            write!(f, " (location: {location})")?;
        }
        Ok(())
    }
}

pub fn tls_connector() -> TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

/// Пишущая половина: отправка фреймов к DC. Разделена через Mutex —
/// насосы моста и ответы на ping идут из разных задач.
pub struct WsWriter {
    write: Mutex<WriteHalf<TlsStream<TcpStream>>>,
}

impl WsWriter {
    /// Отправка бинарного сообщения одним фреймом.
    pub async fn send(&self, data: &[u8]) -> std::io::Result<()> {
        tokio::time::timeout(WS_WRITE_TIMEOUT, async {
            let mut write = self.write.lock().await;
            send_frame(&mut *write, OP_BINARY, data).await?;
            write.flush().await
        })
        .await
        .map_err(|_| write_timeout_error())?
    }

    /// Пакеты одного чанка уходят очередью фреймов с одним flush.
    pub async fn send_batch(&self, mut parts: Vec<Vec<u8>>) -> std::io::Result<()> {
        tokio::time::timeout(WS_WRITE_TIMEOUT, async {
            let mut write = self.write.lock().await;
            send_owned_frames(&mut *write, OP_BINARY, &mut parts).await?;
            write.flush().await
        })
        .await
        .map_err(|_| write_timeout_error())?
    }

    pub async fn close(&self) {
        let _ = tokio::time::timeout(WS_WRITE_TIMEOUT, async {
            let mut write = self.write.lock().await;
            send_frame(&mut *write, OP_CLOSE, &[]).await?;
            write.flush().await
        })
        .await;
    }

    async fn send_pong(&self, payload: &[u8]) {
        let _ = tokio::time::timeout(WS_WRITE_TIMEOUT, async {
            let mut write = self.write.lock().await;
            send_frame(&mut *write, OP_PONG, payload).await?;
            write.flush().await
        })
        .await;
    }
}

fn write_timeout_error() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::TimedOut, "websocket write timed out")
}

pub(crate) fn configure_tcp_keepalive(stream: &TcpStream) -> std::io::Result<()> {
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(TCP_KEEPALIVE_IDLE)
        .with_interval(TCP_KEEPALIVE_INTERVAL);
    socket2::SockRef::from(stream).set_tcp_keepalive(&keepalive)
}

pub struct WsReader {
    read: BufReader<ReadHalf<TlsStream<TcpStream>>>,
    writer: Arc<WsWriter>,
}

impl WsReader {
    /// Следующее бинарное сообщение (с реассемблированием фрагментов и
    /// ответом на пинги). `None` — соединение закрыто любой из сторон.
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        let mut fragment = Vec::new();
        let mut fragmenting = false;
        loop {
            let frame = read_frame(&mut self.read).await?;
            match frame.opcode {
                OP_BINARY => {
                    if fragmenting {
                        return None; // новый дата-фрейм посреди фрагментации
                    }
                    if frame.fin {
                        return Some(frame.payload);
                    }
                    fragmenting = true;
                    fragment = frame.payload;
                }
                OP_CONT => {
                    if !fragmenting {
                        return None; // continuation без начального data frame
                    }
                    if !fragment_length_is_valid(fragment.len(), frame.payload.len()) {
                        return None;
                    }
                    fragment.extend_from_slice(&frame.payload);
                    if frame.fin {
                        return Some(std::mem::take(&mut fragment));
                    }
                }
                OP_PING => {
                    let pong = frame.payload[..frame.payload.len().min(125)].to_vec();
                    self.writer.send_pong(&pong).await;
                }
                OP_PONG => {}
                OP_CLOSE => {
                    let code = if frame.payload.len() >= 2 {
                        u16::from_be_bytes([frame.payload[0], frame.payload[1]])
                    } else {
                        1005
                    };
                    crate::logger::info(format!("ws close received, code {code}"));
                    self.writer.close().await;
                    return None;
                }
                _ => return None,
            }
        }
    }
}

fn fragment_length_is_valid(current: usize, incoming: usize) -> bool {
    current <= MAX_MESSAGE_LEN && incoming <= MAX_MESSAGE_LEN - current
}

/// TCP к `ip:443`, TLS с SNI/верификацией `domain`, HTTP Upgrade на
/// `path` с сабпротоколом `binary`. Возвращает обе половины соединения.
pub async fn connect(
    ip: &str,
    domain: &str,
    path: &str,
    timeout: std::time::Duration,
    connector: &TlsConnector,
) -> Result<(WsReader, Arc<WsWriter>), ConnectError> {
    let deadline = tokio::time::Instant::now() + timeout;
    let tcp = tokio::time::timeout_at(deadline, TcpStream::connect((ip, 443)))
        .await
        .map_err(|_| ConnectError::Timeout)?
        .map_err(ConnectError::Io)?;
    let _ = tcp.set_nodelay(true);
    let _ = configure_tcp_keepalive(&tcp);

    let server_name =
        rustls::pki_types::ServerName::try_from(domain.to_string()).map_err(|error| {
            ConnectError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid server name: {error}"),
            ))
        })?;
    let tls = tokio::time::timeout_at(deadline, connector.connect(server_name, tcp))
        .await
        .map_err(|_| ConnectError::Timeout)?
        .map_err(ConnectError::Tls)?;

    let (read_half, write_half) = tokio::io::split(tls);
    let mut read = BufReader::new(read_half);
    let mut write = write_half;

    let mut key_bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut key_bytes);
    let ws_key = base64::engine::general_purpose::STANDARD.encode(key_bytes);
    let request = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {domain}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {ws_key}\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Protocol: binary\r\n\
         \r\n"
    );

    let headers_raw = tokio::time::timeout_at(deadline, async {
        write.write_all(request.as_bytes()).await?;
        write.flush().await?;

        // Заголовки ответа до пустой строки, с разумным лимитом.
        let mut headers_raw = Vec::with_capacity(1024);
        loop {
            let mut byte = [0u8; 1];
            let n = read.read(&mut byte).await?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "eof during ws handshake",
                ));
            }
            headers_raw.push(byte[0]);
            if headers_raw.len() > 8192 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "ws handshake response too large",
                ));
            }
            if headers_raw.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        Ok(headers_raw)
    })
    .await
    .map_err(|_| ConnectError::Timeout)?
    .map_err(ConnectError::Io)?;

    validate_server_handshake(&headers_raw, &ws_key)?;

    let writer = Arc::new(WsWriter {
        write: Mutex::new(write),
    });
    Ok((
        WsReader {
            read,
            writer: writer.clone(),
        },
        writer,
    ))
}

fn validate_server_handshake(headers_raw: &[u8], ws_key: &str) -> Result<(), ConnectError> {
    let text = std::str::from_utf8(headers_raw).map_err(|error| {
        ConnectError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("non-UTF8 websocket response: {error}"),
        ))
    })?;
    let mut lines = text.split("\r\n");
    let first_line = lines.next().unwrap_or_default();
    let status_code: u16 = first_line
        .split_ascii_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let mut location = None;
    let mut upgrade = None;
    let mut connection = None;
    let mut accept = None;
    let mut protocol = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(invalid_handshake("malformed HTTP header"));
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("location") {
            location = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("upgrade") {
            upgrade = Some(value);
        } else if name.eq_ignore_ascii_case("connection") {
            connection = Some(value);
        } else if name.eq_ignore_ascii_case("sec-websocket-accept") {
            accept = Some(value);
        } else if name.eq_ignore_ascii_case("sec-websocket-protocol") {
            protocol = Some(value);
        }
    }
    if status_code != 101 {
        return Err(ConnectError::Handshake(WsHandshakeError {
            status_code,
            status_line: first_line.to_string(),
            location,
        }));
    }
    if !first_line.starts_with("HTTP/1.1 ") {
        return Err(invalid_handshake("websocket response is not HTTP/1.1"));
    }
    let has_token = |value: Option<&str>, expected: &str| {
        value.is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case(expected))
        })
    };
    if !has_token(upgrade, "websocket") {
        return Err(invalid_handshake("missing Upgrade: websocket"));
    }
    if !has_token(connection, "upgrade") {
        return Err(invalid_handshake("missing Connection: Upgrade"));
    }
    let expected_accept = websocket_accept(ws_key);
    if accept != Some(expected_accept.as_str()) {
        return Err(invalid_handshake("invalid Sec-WebSocket-Accept"));
    }
    if !protocol.is_some_and(|value| value.eq_ignore_ascii_case("binary")) {
        return Err(invalid_handshake(
            "server did not select binary subprotocol",
        ));
    }
    Ok(())
}

fn invalid_handshake(message: &str) -> ConnectError {
    ConnectError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.to_string(),
    ))
}

fn websocket_accept(ws_key: &str) -> String {
    let mut sha1 = Sha1::new();
    sha1.update(ws_key.as_bytes());
    sha1.update(WS_GUID);
    base64::engine::general_purpose::STANDARD.encode(sha1.finalize())
}

struct Frame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

async fn send_frame<W>(write: &mut W, opcode: u8, payload: &[u8]) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let mut frame = Vec::with_capacity(payload.len() + 14);
    encode_frame(&mut frame, opcode, payload);
    write.write_all(&frame).await
}

/// Отправляет принадлежащие writer'у payload'ы без полной копии WebSocket-фрейма.
/// После маскирования эти буферы больше не нужны вызывающему коду, поэтому их
/// можно безопасно изменить на месте и сразу освободить после записи.
async fn send_owned_frames<W>(
    write: &mut W,
    opcode: u8,
    payloads: &mut [Vec<u8>],
) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    for payload in payloads {
        send_owned_frame(write, opcode, payload).await?;
    }
    Ok(())
}

async fn send_owned_frame<W>(write: &mut W, opcode: u8, payload: &mut [u8]) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let mut mask = [0u8; 4];
    rand::rngs::OsRng.fill_bytes(&mut mask);
    apply_mask(payload, &mask);

    let (header, header_len) = frame_header(opcode, payload.len(), mask);
    write_frame_parts(write, &header[..header_len], payload).await
}

async fn write_frame_parts<W>(write: &mut W, header: &[u8], payload: &[u8]) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    let slices = [IoSlice::new(header), IoSlice::new(payload)];
    let written = write.write_vectored(&slices).await?;
    if written == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "failed to write websocket frame",
        ));
    }

    if written < header.len() {
        write.write_all(&header[written..]).await?;
        write.write_all(payload).await
    } else {
        write.write_all(&payload[written - header.len()..]).await
    }
}

/// Кодирование клиентского фрейма: FIN=1, маска обязательна для
/// клиентских фреймов по RFC 6455.
pub fn encode_frame(out: &mut Vec<u8>, opcode: u8, payload: &[u8]) {
    let mut mask = [0u8; 4];
    rand::rngs::OsRng.fill_bytes(&mut mask);

    let (header, header_len) = frame_header(opcode, payload.len(), mask);
    out.extend_from_slice(&header[..header_len]);
    let start = out.len();
    out.extend_from_slice(payload);
    apply_mask(&mut out[start..], &mask);
}

fn frame_header(opcode: u8, payload_len: usize, mask: [u8; 4]) -> ([u8; 14], usize) {
    let mut header = [0u8; 14];
    header[0] = 0x80 | opcode;
    let mut header_len = 2;
    if payload_len < 126 {
        header[1] = 0x80 | payload_len as u8;
    } else if payload_len <= u16::MAX as usize {
        header[1] = 0x80 | 126;
        header[2..4].copy_from_slice(&(payload_len as u16).to_be_bytes());
        header_len = 4;
    } else {
        header[1] = 0x80 | 127;
        header[2..10].copy_from_slice(&(payload_len as u64).to_be_bytes());
        header_len = 10;
    }
    header[header_len..header_len + mask.len()].copy_from_slice(&mask);
    header_len += mask.len();
    (header, header_len)
}

fn apply_mask(payload: &mut [u8], mask: &[u8; 4]) {
    for chunk in payload.chunks_mut(mask.len()) {
        for (byte, mask_byte) in chunk.iter_mut().zip(mask) {
            *byte ^= mask_byte;
        }
    }
}

async fn read_frame<R>(read: &mut R) -> Option<Frame>
where
    R: AsyncReadExt + Unpin,
{
    let mut header = [0u8; 2];
    read.read_exact(&mut header).await.ok()?;
    let fin = header[0] & 0x80 != 0;
    if header[0] & 0x70 != 0 {
        return None; // RSV-биты: расширения не согласованы
    }
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    if masked {
        return None; // сервер не имеет права маскировать свои фреймы
    }
    let len7 = (header[1] & 0x7f) as usize;

    let payload_len = match len7 {
        126 => {
            let mut extended = [0u8; 2];
            read.read_exact(&mut extended).await.ok()?;
            u16::from_be_bytes(extended) as usize
        }
        127 => {
            let mut extended = [0u8; 8];
            read.read_exact(&mut extended).await.ok()?;
            if extended[0] & 0x80 != 0 {
                return None;
            }
            let len = usize::try_from(u64::from_be_bytes(extended)).ok()?;
            if len > MAX_MESSAGE_LEN {
                return None;
            }
            len
        }
        other => other,
    };
    // Контрольные фреймы не бывают больше 125 байт и не фрагментируются.
    if opcode >= 0x8 && (payload_len > 125 || !fin) {
        return None;
    }

    if payload_len > MAX_MESSAGE_LEN {
        return None;
    }
    let mut payload = vec![0u8; payload_len];
    read.read_exact(&mut payload).await.ok()?;
    Some(Frame {
        fin,
        opcode,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::AsyncWrite;

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl AsyncWrite for CountingWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            data: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.bytes.extend_from_slice(data);
            Poll::Ready(Ok(data.len()))
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            self.flushes += 1;
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    struct PartialWriter {
        bytes: Vec<u8>,
        max_write: usize,
    }

    impl AsyncWrite for PartialWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            data: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            let count = data.len().min(self.max_write);
            self.bytes.extend_from_slice(&data[..count]);
            Poll::Ready(Ok(count))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    struct VectoredPartialWriter {
        bytes: Vec<u8>,
        first_write_limit: usize,
        vectored_calls: usize,
    }

    impl AsyncWrite for VectoredPartialWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            data: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.bytes.extend_from_slice(data);
            Poll::Ready(Ok(data.len()))
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buffers: &[IoSlice<'_>],
        ) -> Poll<std::io::Result<usize>> {
            self.vectored_calls += 1;
            let available: usize = buffers.iter().map(|buffer| buffer.len()).sum();
            let mut remaining = self.first_write_limit.min(available);
            let written = remaining;
            for buffer in buffers {
                let count = remaining.min(buffer.len());
                self.bytes.extend_from_slice(&buffer[..count]);
                remaining -= count;
                if remaining == 0 {
                    break;
                }
            }
            Poll::Ready(Ok(written))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn is_write_vectored(&self) -> bool {
            true
        }
    }

    fn encrypted_req_pq(relay: &[u8; crate::obf2::HANDSHAKE_LEN]) -> Vec<u8> {
        use rand::RngCore as _;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch");
        let fraction = ((u64::from(now.subsec_nanos())) << 32) / 1_000_000_000;
        let message_id = ((now.as_secs() << 32) | fraction) & !3;

        let mut packet = Vec::with_capacity(41);
        packet.push(0x0a); // abridged length: 40 bytes / 4
        packet.extend_from_slice(&0u64.to_le_bytes()); // auth_key_id: unencrypted
        packet.extend_from_slice(&message_id.to_le_bytes());
        packet.extend_from_slice(&20u32.to_le_bytes());
        packet.extend_from_slice(&0xbe7e8ef1u32.to_le_bytes());
        let mut nonce = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        packet.extend_from_slice(&nonce);

        let key: [u8; 32] = relay[8..40].try_into().unwrap();
        let iv: [u8; 16] = relay[40..56].try_into().unwrap();
        let mut cipher = crate::crypto::CtrCipher::new(&key, &iv);
        cipher.skip_64();
        cipher.apply(&mut packet);
        packet
    }

    /// Синхронное декодирование полного набора фреймов из буфера.
    fn decode_frames(buffer: &[u8]) -> Vec<Frame> {
        let mut offset = 0;
        let mut frames = Vec::new();
        while offset < buffer.len() {
            let fin = buffer[offset] & 0x80 != 0;
            let opcode = buffer[offset] & 0x0f;
            let masked = buffer[offset + 1] & 0x80 != 0;
            let len7 = (buffer[offset + 1] & 0x7f) as usize;
            let mut cursor = offset + 2;
            let payload_len = match len7 {
                126 => {
                    let len =
                        u16::from_be_bytes(buffer[cursor..cursor + 2].try_into().unwrap()) as usize;
                    cursor += 2;
                    len
                }
                127 => {
                    let len =
                        u64::from_be_bytes(buffer[cursor..cursor + 8].try_into().unwrap()) as usize;
                    cursor += 8;
                    len
                }
                other => other,
            };
            let mut mask: Option<[u8; 4]> = None;
            if masked {
                mask = Some(buffer[cursor..cursor + 4].try_into().unwrap());
                cursor += 4;
            }
            let mut payload = buffer[cursor..cursor + payload_len].to_vec();
            if let Some(mask) = mask {
                for (i, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[i % 4];
                }
            }
            cursor += payload_len;
            frames.push(Frame {
                fin,
                opcode,
                payload,
            });
            offset = cursor;
        }
        frames
    }

    #[test]
    fn encode_decode_roundtrip_small_payload() {
        let mut buffer = Vec::new();
        encode_frame(&mut buffer, OP_BINARY, b"hello");
        let frames = decode_frames(&buffer);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].fin);
        assert_eq!(frames[0].opcode, OP_BINARY);
        assert_eq!(frames[0].payload, b"hello");
    }

    #[test]
    fn encode_decode_extended_lengths() {
        for len in [125usize, 126, 65535, 65536, 70000] {
            let payload = vec![0xa5; len];
            let mut buffer = Vec::new();
            encode_frame(&mut buffer, OP_BINARY, &payload);
            let frames = decode_frames(&buffer);
            assert_eq!(frames.len(), 1, "len {len}");
            assert_eq!(frames[0].payload, payload, "len {len}");
        }
    }

    #[test]
    fn multiple_frames_in_one_buffer() {
        let mut buffer = Vec::new();
        encode_frame(&mut buffer, OP_BINARY, b"one");
        encode_frame(&mut buffer, OP_PING, b"");
        encode_frame(&mut buffer, OP_BINARY, b"two");
        let frames = decode_frames(&buffer);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].payload, b"one");
        assert_eq!(frames[1].opcode, OP_PING);
        assert_eq!(frames[2].payload, b"two");
    }

    #[tokio::test]
    async fn batched_frames_are_committed_with_one_flush() {
        let mut write = CountingWriter::default();
        let mut parts = vec![b"one".to_vec(), b"two".to_vec(), b"three".to_vec()];
        send_owned_frames(&mut write, OP_BINARY, &mut parts)
            .await
            .unwrap();

        assert_eq!(write.flushes, 0, "frame writes must not flush individually");
        write.flush().await.unwrap();
        assert_eq!(write.flushes, 1, "the batch must be committed once");

        let frames = decode_frames(&write.bytes);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].payload, b"one");
        assert_eq!(frames[1].payload, b"two");
        assert_eq!(frames[2].payload, b"three");
    }

    #[tokio::test]
    async fn owned_frame_roundtrips_all_length_encodings_without_a_frame_copy() {
        for len in [0usize, 125, 126, 65_535, 65_536, 1024 * 1024] {
            let original: Vec<u8> = (0..len).map(|index| index as u8).collect();
            let mut payload = original.clone();
            let mut write = CountingWriter::default();

            send_owned_frame(&mut write, OP_BINARY, &mut payload)
                .await
                .unwrap();

            let frames = decode_frames(&write.bytes);
            assert_eq!(frames.len(), 1, "len {len}");
            assert_eq!(frames[0].payload, original, "len {len}");
        }
    }

    #[tokio::test]
    async fn owned_frame_completes_after_partial_vectored_writes() {
        let original: Vec<u8> = (0..257).map(|index| index as u8).collect();
        let mut payload = original.clone();
        let mut write = PartialWriter {
            bytes: Vec::new(),
            max_write: 3,
        };

        send_owned_frame(&mut write, OP_BINARY, &mut payload)
            .await
            .unwrap();

        let frames = decode_frames(&write.bytes);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, original);
    }

    #[tokio::test]
    async fn owned_frame_handles_vectored_writes_at_and_inside_payload_boundary() {
        // Payload 257 использует 8-байтный WS-заголовок: 4 байта длины и mask.
        for first_write_limit in [8usize, 8 + 17] {
            let original: Vec<u8> = (0..257).map(|index| index as u8).collect();
            let mut payload = original.clone();
            let mut write = VectoredPartialWriter {
                bytes: Vec::new(),
                first_write_limit,
                vectored_calls: 0,
            };

            send_owned_frame(&mut write, OP_BINARY, &mut payload)
                .await
                .unwrap();

            assert_eq!(write.vectored_calls, 1);
            let frames = decode_frames(&write.bytes);
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0].payload, original);
        }
    }

    #[test]
    fn relay_init_sized_frame_roundtrip() {
        // 64-байтный relay init — первый фрейм сессии.
        let payload = vec![0x33u8; 64];
        let mut buffer = Vec::new();
        encode_frame(&mut buffer, OP_BINARY, &payload);
        let frames = decode_frames(&buffer);
        assert_eq!(frames[0].payload, payload);
    }

    #[test]
    fn validates_rfc6455_upgrade_headers() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let valid = b"HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: keep-alive, Upgrade\r\n\
Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
Sec-WebSocket-Protocol: binary\r\n\r\n";
        assert!(validate_server_handshake(valid, key).is_ok());

        let invalid = b"HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Accept: wrong\r\n\
Sec-WebSocket-Protocol: binary\r\n\r\n";
        let error = validate_server_handshake(invalid, key).unwrap_err();
        assert!(error.to_string().contains("Sec-WebSocket-Accept"));
    }

    #[tokio::test]
    async fn rejects_masked_server_frames() {
        let (mut write, mut read) = tokio::io::duplex(128);
        let mut masked = Vec::new();
        encode_frame(&mut masked, OP_BINARY, b"masked");
        write.write_all(&masked).await.unwrap();
        drop(write);
        assert!(read_frame(&mut read).await.is_none());
    }

    #[test]
    fn fragmented_message_is_rejected_before_exceeding_the_limit() {
        assert!(fragment_length_is_valid(MAX_MESSAGE_LEN - 1, 1));
        assert!(!fragment_length_is_valid(MAX_MESSAGE_LEN - 1, 2));
        assert!(!fragment_length_is_valid(MAX_MESSAGE_LEN, 1));
        assert!(!fragment_length_is_valid(MAX_MESSAGE_LEN + 1, 0));
    }

    /// Живая проверка против настоящего DC2 Telegram: WS-хендшейк с
    /// верифицированным TLS, отправка relay init + настоящего req_pq_multi,
    /// ожидание непустого ответа DC. Требует сеть — запускать явно:
    /// `cargo test live_dc -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires network access to Telegram DC"]
    async fn live_dc2_relay_handshake() {
        let connector = tls_connector();
        let (mut reader, writer) = connect(
            "149.154.167.220",
            "kws2.web.telegram.org",
            "/apiws",
            std::time::Duration::from_secs(10),
            &connector,
        )
        .await
        .expect("ws connect to DC2 with verified TLS");

        let relay = crate::obf2::make_relay_init(
            crate::obf2::ProtoTag::Abridged,
            2,
            &mut rand::rngs::OsRng,
        );
        writer.send(&relay).await.expect("send relay init");
        writer
            .send_batch(vec![encrypted_req_pq(&relay)])
            .await
            .expect("send req_pq_multi");

        let response = tokio::time::timeout(std::time::Duration::from_secs(10), reader.recv())
            .await
            .expect("timeout waiting for resPQ")
            .expect("DC closed connection without resPQ");
        assert!(!response.is_empty(), "DC returned an empty response");
        writer.close().await;
    }
}
