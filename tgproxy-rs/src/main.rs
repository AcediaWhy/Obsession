//! obsession-tg-proxy — headless MTProto<->WebSocket мост для Telegram.
//!
//! Rust-переписывание CLI-части tg-ws-proxy (Flowseal, MIT). Контракт
//! запуска совместим с Obsession (`ObsessionTauri/src-tauri/src/proxy.rs`):
//! флаги `--port/--secret/--fake-tls-domain/--cfproxy-cache` и строка-сигнал
//! готовности `tg://proxy?...` одной строкой без пробелов в stderr.

mod bridge;
mod cfrelay;
mod cli;
mod connect_race;
mod crypto;
mod dc;
mod dns;
mod fake_tls;
mod link;
mod logger;
mod obf2;
mod splitter;
mod upstream;
mod ws;

use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::Semaphore;

const MAX_CONCURRENT_CLIENTS: usize = 512;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match cli::parse(&argv) {
        Ok(cli::Parsed::Run(args)) => args,
        Ok(cli::Parsed::Help(text)) | Ok(cli::Parsed::Version(text)) => {
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            // Код 2 — как у argparse апстрима: Obsession классифицирует
            // ранний выход как фатальную ошибку запуска.
            eprintln!("error: {error}\n\n{}", cli::USAGE.trim_end());
            return ExitCode::from(2);
        }
    };

    let secret = match decode_secret(&args.secret) {
        Some(secret) => secret,
        None => {
            eprintln!("error: secret must be 32 hex chars (16 bytes)");
            return ExitCode::from(2);
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: cannot start tokio runtime: {error}");
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(run(args, secret)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

async fn run(args: cli::Args, secret: [u8; 16]) -> Result<(), String> {
    let listener = TcpListener::bind((args.host.as_str(), args.port))
        .await
        .map_err(|error| format!("bind {}:{}: {error}", args.host, args.port))?;

    let cf_relay: Option<Arc<cfrelay::CfRelay>> = if args.no_cfproxy {
        None
    } else {
        Some(Arc::new(cfrelay::CfRelay::new(
            args.cfproxy_cache.as_deref(),
        )))
    };
    let worker_domains = Arc::new(args.cfproxy_worker_domains.clone());

    logger::info("  Obsession Telegram Proxy (rust)");
    logger::info(format!("  Listening on   {}:{}", args.host, args.port));
    logger::info("  Secret:        [redacted]");
    if let Some(domain) = &args.fake_tls_domain {
        logger::info(format!("  Fake TLS:      {domain}"));
    }
    if args.no_direct {
        logger::warn("  Direct DC:     disabled (--no-direct debug flag)");
    }
    if !worker_domains.is_empty() {
        logger::warn(format!(
            "  CF worker:     ignored (unsupported architecture; configured: {})",
            worker_domains.join(", ")
        ));
    }
    logger::info(format!(
        "  CF proxy:      {}",
        if cf_relay.is_some() {
            "enabled (auto)"
        } else {
            "disabled"
        }
    ));

    // Контракт Obsession: готовность определяется первой строкой, совпавшей
    // с regex `tg://proxy\?[^\s]+` в stdout/stderr. Печатаем без
    // лог-префиксов и с завершающим flush.
    let host_for_link = link_host(&args.host).await;
    let tg_link = link::build_link(
        &host_for_link,
        args.port,
        &args.secret,
        args.fake_tls_domain.as_deref(),
    );
    eprintln!("{tg_link}");

    let connector = ws::tls_connector();
    let ctx = Arc::new(bridge::BridgeContext {
        secret,
        connector,
        cf_relay,
        worker_domains,
        no_direct: args.no_direct,
        masking: args.fake_tls_domain.clone(),
    });
    let client_slots = Arc::new(Semaphore::new(MAX_CONCURRENT_CLIENTS));
    let rejected_clients = Arc::new(AtomicU64::new(0));

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    match client_slots.clone().try_acquire_owned() {
                        Ok(permit) => {
                            let ctx = ctx.clone();
                            tokio::spawn(async move {
                                let _permit = permit;
                                bridge::handle_client(stream, ctx).await;
                            });
                        }
                        Err(_) => {
                            // Drop закрывает принятый сокет немедленно. Логируем
                            // первый и каждый 128-й отказ, чтобы флуд не раздувал лог.
                            let rejected = rejected_clients.fetch_add(1, Ordering::Relaxed) + 1;
                            if rejected == 1 || rejected.is_multiple_of(128) {
                                logger::warn(format!(
                                    "connection limit reached ({MAX_CONCURRENT_CLIENTS}); rejected {rejected} clients"
                                ));
                            }
                            drop(stream);
                        }
                    }
                }
                Err(error) => logger::warn(format!("accept: {error}")),
            },
            _ = tokio::signal::ctrl_c() => {
                logger::info("shutting down");
                return Ok(());
            }
        }
    }
}

fn decode_secret(hex: &str) -> Option<[u8; 16]> {
    let bytes = hex.as_bytes();
    let mut out = [0u8; 16];
    for i in 0..16 {
        let high = (bytes[i * 2] as char).to_digit(16)?;
        let low = (bytes[i * 2 + 1] as char).to_digit(16)?;
        out[i] = (high * 16 + low) as u8;
    }
    Some(out)
}

/// Хост для ссылки: при листене на 0.0.0.0 подставляем локальный IP
/// (как get_link_host апстрима). UDP-connect маршрут выбирает, но пакеты
/// не отправляет.
async fn link_host(host: &str) -> String {
    if host != "0.0.0.0" && host != "::" {
        return host.to_string();
    }
    let local_ip = async {
        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect("8.8.8.8:80").await?;
        Ok::<_, std::io::Error>(socket.local_addr()?.ip().to_string())
    };
    match local_ip.await {
        Ok(ip) => ip,
        Err(_) => host.to_string(),
    }
}
