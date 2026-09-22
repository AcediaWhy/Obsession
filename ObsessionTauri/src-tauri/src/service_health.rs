//! Application-layer checks shared by interactive and protected probes.
use std::time::Duration;

pub const DISCORD_UPDATE_PATH: &str =
    "/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64";
pub const DISCORD_API_PATH: &str = "/api/v10/gateway";
pub const DISCORD_HEALTH_HOSTS: [&str; 2] = ["updates.discord.com", "discord.com"];
const BODY_LIMIT: usize = 256 * 1024;

pub fn probe_hosts(category: &str, allowed: &[&str], limit: usize) -> Vec<String> {
    if category == "discord" {
        let hosts = DISCORD_HEALTH_HOSTS
            .into_iter()
            .filter(|host| {
                allowed.iter().any(|suffix| {
                    *host == *suffix
                        || host
                            .strip_suffix(*suffix)
                            .is_some_and(|prefix| prefix.ends_with('.'))
                })
            })
            .take(limit)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !hosts.is_empty() {
            return hosts;
        }
    }
    allowed
        .iter()
        .take(limit)
        .map(|host| (*host).to_owned())
        .collect()
}

pub fn discord_path(host: &str) -> Option<&'static str> {
    match host {
        "updates.discord.com" => Some(DISCORD_UPDATE_PATH),
        "discord.com" => Some(DISCORD_API_PATH),
        _ => None,
    }
}

pub fn service_url(mut url: reqwest::Url) -> reqwest::Url {
    if let Some(path) = url.host_str().and_then(discord_path) {
        url = url.join(path).expect("constant absolute path");
    }
    url
}

fn validate_discord_body(host: &str, status: u16, body: &[u8]) -> Result<(), String> {
    if status != 200 {
        return Err(format!("service returned HTTP {status}"));
    }
    let json: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| "service returned invalid JSON".to_string())?;
    let valid = match host {
        "discord.com" => json
            .get("url")
            .and_then(|v| v.as_str())
            .is_some_and(|v| v == "wss://gateway.discord.gg" || v == "wss://gateway.discord.gg/"),
        "updates.discord.com" => {
            json.pointer("/full/host_version")
                .and_then(|v| v.as_array())
                .is_some_and(|v| !v.is_empty() && v.iter().all(|part| part.as_u64().is_some()))
                && json
                    .pointer("/full/url")
                    .and_then(|v| v.as_str())
                    .is_some_and(|v| {
                        reqwest::Url::parse(v).ok().is_some_and(|u| {
                            u.scheme() == "https"
                                && u.host_str().is_some_and(|h| h.ends_with(".discordapp.net"))
                        })
                    })
        }
        _ => false,
    };
    valid
        .then_some(())
        .ok_or_else(|| "service response is not a valid gateway/update manifest".into())
}

/// A ServerHello or response headers alone cannot confirm a working service.
/// Discord needs a complete valid document; generic endpoints need bounded
/// body progress (or a complete empty response, e.g. connectivity HTTP 204).
pub async fn read_response(
    mut response: reqwest::Response,
    timeout: Duration,
) -> Result<usize, String> {
    let host = response.url().host_str().unwrap_or_default().to_owned();
    let status = response.status().as_u16();
    let strict = discord_path(&host).is_some();
    tokio::time::timeout(timeout, async move {
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("response body: {e}"))?
        {
            if chunk.len() > BODY_LIMIT.saturating_sub(body.len()) {
                if strict {
                    return Err("service document exceeds probe limit".into());
                }
                return Ok(BODY_LIMIT);
            }
            body.extend_from_slice(&chunk);
            if !strict && body.len() == BODY_LIMIT {
                return Ok(body.len());
            }
        }
        if strict {
            validate_discord_body(&host, status, &body)?;
        }
        Ok(body.len())
    })
    .await
    .map_err(|_| "response body timeout".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn error_pages_and_empty_handshakes_do_not_confirm_discord() {
        assert!(validate_discord_body("updates.discord.com", 400, b"{}").is_err());
        assert!(
            validate_discord_body("updates.discord.com", 200, b"<html>blocked</html>").is_err()
        );
        assert!(validate_discord_body("discord.com", 200, b"").is_err());
        assert!(validate_discord_body(
            "discord.com",
            200,
            br#"{"url":"wss://gateway.discord.gg"}"#
        )
        .is_ok());
        assert!(validate_discord_body("updates.discord.com", 200, br#"{"full":{"host_version":[1,0,1],"url":"https://stable.dl2.discordapp.net/full.distro"}}"#).is_ok());
    }
    #[test]
    fn updater_probe_has_required_platform_parameters() {
        let url = service_url(
            reqwest::Url::parse("https://updates.discord.com/?obsession_recovery_probe=1").unwrap(),
        );
        assert_eq!(url.path(), "/distributions/app/manifests/latest");
        assert_eq!(url.query(), Some("channel=stable&platform=win&arch=x64"));
    }

    #[test]
    fn health_targets_are_relevant_and_stay_within_the_selected_config() {
        assert_eq!(
            probe_hosts(
                "discord",
                &["betterdiscord.app", "cdn.discordapp.com", "discord.com"],
                2
            ),
            vec!["updates.discord.com", "discord.com"]
        );
        assert_eq!(
            probe_hosts("discord", &["cdn.discordapp.com"], 2),
            vec!["cdn.discordapp.com"]
        );
        assert_eq!(
            probe_hosts("video", &["one.example", "two.example"], 1),
            vec!["one.example"]
        );
    }

    #[tokio::test]
    async fn body_truncation_and_post_header_stall_are_failures() {
        use std::io::{Read, Write};
        for stall in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let worker = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 2048];
                socket.read(&mut request).unwrap();
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nx",
                    )
                    .unwrap();
                if stall {
                    std::thread::sleep(Duration::from_millis(250));
                }
            });
            let response = reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .get(format!("http://{address}/"))
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .unwrap();
            assert!(read_response(response, Duration::from_millis(100))
                .await
                .is_err());
            worker.join().unwrap();
        }
    }
}
