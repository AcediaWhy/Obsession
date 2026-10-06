use obsession_tls_record_proxy::{bind, run, Config};
use std::{io, net::SocketAddr};

const USAGE: &str = "Usage: obsession-tls-record-proxy --target IP:PORT --server-name DNS_NAME [--listen 127.0.0.1:18443] [--fragment-bytes 512]\n\nFixed-target raw TLS TCP adapter; client must send TLS directly and retain the original server name. No CONNECT, TLS decryption, DNS changes, or system proxy changes.";

fn arguments() -> Result<Option<Config>, String> {
    let mut listen = "127.0.0.1:18443".parse::<SocketAddr>().unwrap();
    let mut target = None;
    let mut name = None;
    let mut fragment_bytes = 512;
    let mut seen = std::collections::BTreeSet::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--help" || argument == "-h" {
            return Ok(None);
        }
        if !seen.insert(argument.clone()) {
            return Err(format!("duplicate argument: {argument}"));
        }
        if !["--listen", "--target", "--server-name", "--fragment-bytes"]
            .contains(&argument.as_str())
        {
            return Err(format!("unknown argument: {argument}"));
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {argument}"))?;
        match argument.as_str() {
            "--listen" => {
                listen = value
                    .parse()
                    .map_err(|_| "listen must be a numeric IP:port")?
            }
            "--target" => {
                target = Some(
                    value
                        .parse()
                        .map_err(|_| "target must be a numeric IP:port")?,
                )
            }
            "--server-name" => name = Some(value),
            "--fragment-bytes" => {
                fragment_bytes = value
                    .parse()
                    .map_err(|_| "fragment size must be an integer")?
            }
            _ => unreachable!(),
        }
    }
    let mut cfg = Config::new(
        listen,
        target.ok_or("--target is required")?,
        name.ok_or("--server-name is required")?,
    );
    cfg.fragment_bytes = fragment_bytes;
    cfg.log_events = true;
    cfg.validate().map_err(|error| error.to_string())?;
    Ok(Some(cfg))
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let cfg = match arguments() {
        Ok(Some(cfg)) => cfg,
        Ok(None) => {
            println!("{USAGE}");
            return Ok(());
        }
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let listener = bind(&cfg).await?;
    println!(
        "TLS record prototype listening on {}; fixed target {}; SNI {}; fragment cap {} bytes",
        listener.local_addr()?,
        cfg.target,
        cfg.server_name,
        cfg.fragment_bytes
    );
    println!("Prototype limits: 16 connections, 5s hello/connect, 60s idle, 300s maximum connection lifetime.");
    run(listener, cfg, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}
