//! Унифицированный upstream для моста.
//!
//! Telegram `/apiws` — message-oriented транспорт: init и каждый MTProto
//! transport packet должны уходить отдельными WebSocket messages. Raw TCP,
//! SOCKS и будущий PersonalRelay — обычные byte streams, где границы записей
//! не имеют значения. Это различие хранится явно, чтобы splitter нельзя было
//! случайно применить к потоковому маршруту или пропустить для `/apiws`.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::cfrelay::CfRelay;
use crate::ws;

const STREAM_READ_CHUNK: usize = 64 * 1024;
const STREAM_WRITE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    ByteStream,
    TelegramMessages,
}

pub struct Upstream {
    route: &'static str,
    read: UpstreamReader,
    write: UpstreamWriter,
    relay_feedback: Option<RelayFeedback>,
}

#[derive(Clone)]
pub struct RelayFeedback {
    relay: Arc<CfRelay>,
    dc: u16,
    domain: String,
}

impl RelayFeedback {
    pub fn success(&self) {
        self.relay.report_session_success(self.dc, &self.domain);
    }

    pub fn failure(&self) {
        self.relay.report_session_failure(self.dc, &self.domain);
    }
}

impl Upstream {
    pub fn from_tcp(stream: TcpStream, route: &'static str) -> Self {
        let _ = stream.set_nodelay(true);
        let _ = ws::configure_tcp_keepalive(&stream);
        let (read, write) = stream.into_split();
        Self {
            route,
            read: UpstreamReader::ByteStream(read),
            write: UpstreamWriter::ByteStream(Arc::new(Mutex::new(write))),
            relay_feedback: None,
        }
    }

    pub fn from_websocket(
        read: ws::WsReader,
        write: Arc<ws::WsWriter>,
        route: &'static str,
    ) -> Self {
        Self {
            route,
            read: UpstreamReader::TelegramMessages(read),
            write: UpstreamWriter::TelegramMessages(write),
            relay_feedback: None,
        }
    }

    pub fn from_public_relay(
        read: ws::WsReader,
        write: Arc<ws::WsWriter>,
        relay: Arc<CfRelay>,
        dc: u16,
        domain: String,
    ) -> Self {
        Self {
            route: "public-relay-wss",
            read: UpstreamReader::TelegramMessages(read),
            write: UpstreamWriter::TelegramMessages(write),
            relay_feedback: Some(RelayFeedback { relay, dc, domain }),
        }
    }

    pub fn route(&self) -> &'static str {
        self.route
    }

    pub fn relay_feedback(&self) -> Option<RelayFeedback> {
        self.relay_feedback.clone()
    }

    pub async fn send_init(&self, init: &[u8]) -> io::Result<()> {
        self.write.send(init).await
    }

    pub fn into_parts(self) -> (UpstreamReader, UpstreamWriter) {
        (self.read, self.write)
    }
}

pub enum UpstreamReader {
    ByteStream(OwnedReadHalf),
    TelegramMessages(ws::WsReader),
}

impl UpstreamReader {
    pub async fn recv_into(&mut self, buffer: &mut Vec<u8>) -> io::Result<Option<usize>> {
        match self {
            Self::ByteStream(read) => {
                buffer.resize(STREAM_READ_CHUNK, 0);
                let count = read.read(buffer).await?;
                buffer.truncate(count);
                Ok((count != 0).then_some(count))
            }
            Self::TelegramMessages(read) => match read.recv().await {
                Some(data) => {
                    let count = data.len();
                    *buffer = data;
                    Ok(Some(count))
                }
                None => Ok(None),
            },
        }
    }
}

#[derive(Clone)]
pub enum UpstreamWriter {
    ByteStream(Arc<Mutex<OwnedWriteHalf>>),
    TelegramMessages(Arc<ws::WsWriter>),
}

impl UpstreamWriter {
    pub fn framing(&self) -> Framing {
        match self {
            Self::ByteStream(_) => Framing::ByteStream,
            Self::TelegramMessages(_) => Framing::TelegramMessages,
        }
    }

    pub async fn send(&self, data: &[u8]) -> io::Result<()> {
        match self {
            Self::ByteStream(write) => tokio::time::timeout(STREAM_WRITE_TIMEOUT, async {
                write.lock().await.write_all(data).await
            })
            .await
            .map_err(|_| write_timeout_error())?,
            Self::TelegramMessages(write) => write.send(data).await,
        }
    }

    pub async fn send_batch(&self, parts: &[Vec<u8>]) -> io::Result<()> {
        match self {
            Self::ByteStream(write) => tokio::time::timeout(STREAM_WRITE_TIMEOUT, async {
                let mut write = write.lock().await;
                for part in parts {
                    write.write_all(part).await?;
                }
                Ok(())
            })
            .await
            .map_err(|_| write_timeout_error())?,
            Self::TelegramMessages(write) => write.send_batch(parts).await,
        }
    }

    pub async fn close(&self) {
        match self {
            Self::ByteStream(write) => {
                let _ = tokio::time::timeout(STREAM_WRITE_TIMEOUT, async {
                    write.lock().await.shutdown().await
                })
                .await;
            }
            Self::TelegramMessages(write) => write.close().await,
        }
    }
}

fn write_timeout_error() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "upstream write timed out")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn tcp_upstream_is_a_byte_stream_and_round_trips() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = TcpStream::connect(address).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        let upstream = Upstream::from_tcp(client, "test-tcp");

        assert_eq!(upstream.route(), "test-tcp");
        assert_eq!(upstream.write.framing(), Framing::ByteStream);
        upstream.send_init(b"init").await.unwrap();

        let mut received = [0u8; 4];
        server.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"init");
    }
}
