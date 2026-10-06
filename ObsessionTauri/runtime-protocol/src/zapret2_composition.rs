//! A bounded selection of two verified named recipes, never caller-supplied
//! actions, paths or domains. Policy v1 partitions Discord TLS by hostname.
use serde::{Deserialize, Serialize};

const PREFIX: &str = "dc1_";
pub const DISCORD_TLS_PORTS: &str = "443,2053,2083,2087,2096,8443";
pub const CDN_DOMAINS: &[&str] = &[
    "cdn.discordapp.com", "discordapp.net", "discordcdn.com", "discord.media",
    "discord-attachments-uploads-prd.storage.googleapis.com",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscordScope { Api, Cdn }

impl DiscordScope {
    pub fn arguments(self) -> Vec<String> {
        let option = match self { Self::Api => "--hostlist-exclude-domains", Self::Cdn => "--hostlist-domains" };
        vec![format!("{option}={}", CDN_DOMAINS.join(","))]
    }
    pub fn matches_cdn(host: &str) -> bool {
        CDN_DOMAINS.iter().any(|domain| host == *domain || host.strip_suffix(domain).is_some_and(|prefix| prefix.ends_with('.')))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscordComposition {
    pub api_profile_id: String,
    pub cdn_profile_id: String,
}

impl DiscordComposition {
    pub fn new(api: &str, cdn: &str) -> Result<Self, &'static str> {
        let valid = |id: &str| !id.is_empty() && !id.starts_with(PREFIX) && !id.contains("--")
            && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        if api == cdn || !valid(api) || !valid(cdn) || PREFIX.len() + api.len() + cdn.len() + 2 > crate::MAX_STRATEGY_ID_BYTES {
            return Err("invalid Discord composition parents");
        }
        Ok(Self { api_profile_id: api.into(), cdn_profile_id: cdn.into() })
    }
    pub fn id(&self) -> String { format!("{PREFIX}{}--{}", self.api_profile_id, self.cdn_profile_id) }
    pub fn parse(id: &str) -> Result<Option<Self>, &'static str> {
        let Some(parents) = id.strip_prefix(PREFIX) else { return Ok(None); };
        let (api, cdn) = parents.split_once("--").ok_or("missing Discord composition parent")?;
        Self::new(api, cdn).map(Some)
    }
}

pub fn compatible(category: &str, tcp: Option<&str>, udp: Option<&str>, l7: &[String],
    hostlist: Option<&str>, ipset: Option<&str>, transports: &[String]) -> bool {
    category == "discord" && tcp.is_none_or(|value| value == "443" || value == DISCORD_TLS_PORTS) && udp.is_none()
        && l7 == ["tls"] && hostlist.is_none() && ipset.is_none()
        && transports.iter().all(|value| matches!(value.as_str(), "tcp" | "tls"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_both_ids_and_rejects_nested_or_injected_recipes() {
        let recipe = DiscordComposition::new("discord_tls_a", "discord_tls_b").unwrap();
        assert_eq!(DiscordComposition::parse(&recipe.id()).unwrap(), Some(recipe));
        for (api,cdn) in [("a","a"),("dc1_a--b","c"),("a--b","c"),("--lua-desync=evil","b"),("../a","b")] {
            assert!(DiscordComposition::new(api,cdn).is_err());
        }
        assert!(DiscordComposition::parse("dc1_a--b--c").is_err());
    }
    #[test]
    fn cdn_subdomains_have_one_owner_and_suffix_lookalikes_do_not_match() {
        for host in ["cdn.discordapp.com","media.discordapp.net","images-ext-1.discordapp.net","x.cdn.discordapp.com"] {
            assert!(DiscordScope::matches_cdn(host));
        }
        for host in ["discord.com","gateway.discord.gg","evilcdn.discordapp.com","cdn.discordapp.com.evil.invalid"] {
            assert!(!DiscordScope::matches_cdn(host));
        }
        assert!(DiscordScope::Api.arguments()[0].starts_with("--hostlist-exclude-domains="));
    }
    #[test]
    fn incompatible_filters_cannot_be_silently_widened() {
        let l7=vec!["tls".into()];let transports=vec!["tcp".into()];
        assert!(compatible("discord",None,None,&l7,None,None,&transports));
        assert!(!compatible("discord",Some("443,8443"),None,&l7,None,None,&transports));
        assert!(!compatible("discord",None,None,&l7,Some("custom.txt"),None,&transports));
        assert!(!compatible("youtube_twitch",None,None,&l7,None,None,&transports));
    }
}
