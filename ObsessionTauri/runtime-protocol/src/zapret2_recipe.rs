//! Resource-only recipe types shared by the two pack compilers.
//! These types are never accepted as IPC launch parameters: launch requests
//! still select fixed IDs from the protected, verified pack.

use serde::{Deserialize, Serialize};

pub const PACK_SCHEMA_VERSION: u32 = 2;
pub const MAX_STEPS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesyncStep {
    pub action: String,
    /// Explicit per-instance filter; `all` restores the native default.
    pub payload: Vec<String>,
    /// None inherits the profile range, never the preceding step's range.
    #[serde(default)]
    pub out_range: Option<String>,
    #[serde(default)]
    pub in_range: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceEvidence {
    AuthorReport,
    BlockcheckReport,
    Derived,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSource {
    pub label: String,
    pub family: String,
    pub urls: Vec<String>,
    pub evidence: SourceEvidence,
    pub notes: String,
}

impl CandidateSource {
    pub fn valid(&self) -> bool {
        let text = |value: &str, limit| {
            !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
        };
        text(&self.label, 160)
            && text(&self.family, 64)
            && text(&self.notes, 2048)
            && !self.urls.is_empty()
            && self.urls.len() <= 4
            && self.urls.iter().all(|url| {
                url.starts_with("https://github.com/bol-van/")
                    && text(url, 512)
                    && !url.chars().any(char::is_whitespace)
            })
    }
}

pub fn valid_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

/// Bounds and vocabulary are identical in the app and the privileged service.
pub fn validate_steps(desync: &[String], steps: &[DesyncStep]) -> Result<(), &'static str> {
    if steps.is_empty() {
        return Ok(());
    }
    if !desync.is_empty() || steps.len() > MAX_STEPS {
        return Err("recipe must contain either legacy desync or bounded steps");
    }
    let allowed = [
        "all",
        "known",
        "unknown",
        "empty",
        "http_req",
        "tls_client_hello",
        "quic_initial",
        "discord_ip_discovery",
        "stun",
        "wireguard_initiation",
        "wireguard_cookie",
    ];
    for step in steps {
        if !valid_value(&step.action)
            || step.action.starts_with("--")
            || step.payload.is_empty()
            || step.payload.len() > 8
            || step
                .payload
                .iter()
                .any(|value| !allowed.contains(&value.as_str()))
            || step
                .payload
                .iter()
                .enumerate()
                .any(|(index, value)| step.payload[..index].contains(value))
            || (step.payload.len() > 1
                && step
                    .payload
                    .iter()
                    .any(|v| matches!(v.as_str(), "all" | "known")))
            || step.out_range.as_deref().is_some_and(|v| !valid_value(v))
            || step.in_range.as_deref().is_some_and(|v| !valid_value(v))
        {
            return Err("invalid recipe step or execution scope");
        }
    }
    Ok(())
}

/// Preserve old argv byte-for-byte; scoped steps explicitly restore all three
/// filters each time, so the last step cannot leak its scope into the next one.
pub fn arguments(
    payload: &[String],
    out_range: Option<&str>,
    in_range: Option<&str>,
    desync: &[String],
    steps: &[DesyncStep],
) -> Vec<String> {
    let mut result = Vec::new();
    if steps.is_empty() {
        if !payload.is_empty() {
            result.push(format!("--payload={}", payload.join(",")));
        }
        result.extend(desync.iter().map(|action| format!("--lua-desync={action}")));
    } else {
        for step in steps {
            result.push(format!("--payload={}", step.payload.join(",")));
            result.push(format!(
                "--out-range={}",
                step.out_range.as_deref().or(out_range).unwrap_or("a0")
            ));
            result.push(format!(
                "--in-range={}",
                step.in_range.as_deref().or(in_range).unwrap_or("a0")
            ));
            result.push(format!("--lua-desync={}", step.action));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn step(action: &str, payload: &str) -> DesyncStep {
        DesyncStep {
            action: action.into(),
            payload: vec![payload.into()],
            out_range: None,
            in_range: None,
        }
    }
    #[test]
    fn window_action_precedes_clienthello_scope_and_next_step_resets_ranges() {
        let mut window = step("wssize:wsize=1:scale=6", "all");
        window.out_range = Some("<s1".into());
        let steps = [window, step("multidisorder:pos=2", "tls_client_hello")];
        assert!(validate_steps(&[], &steps).is_ok());
        assert_eq!(
            arguments(&[], Some("-d10"), None, &[], &steps),
            [
                "--payload=all",
                "--out-range=<s1",
                "--in-range=a0",
                "--lua-desync=wssize:wsize=1:scale=6",
                "--payload=tls_client_hello",
                "--out-range=-d10",
                "--in-range=a0",
                "--lua-desync=multidisorder:pos=2"
            ]
        );
    }
    #[test]
    fn legacy_arguments_keep_their_original_shape() {
        assert_eq!(
            arguments(
                &["tls_client_hello".into()],
                Some("-d10"),
                None,
                &["multidisorder:pos=midsld".into()],
                &[]
            ),
            [
                "--payload=tls_client_hello",
                "--lua-desync=multidisorder:pos=midsld"
            ]
        );
    }
    #[test]
    fn rejects_mixed_representations_unknown_scopes_and_unbounded_recipes() {
        assert!(validate_steps(&["fake".into()], &[step("fake", "all")]).is_err());
        assert!(validate_steps(&[], &[step("fake", "typo")]).is_err());
        assert!(validate_steps(&[], &[step("fake\n--new", "all")]).is_err());
        assert!(validate_steps(&[], &vec![step("fake", "all"); MAX_STEPS + 1]).is_err());
        let mut invalid = step("fake", "all");
        invalid.payload.push("empty".into());
        assert!(validate_steps(&[], &[invalid]).is_err());
    }

    #[test]
    fn resource_metadata_is_bounded_and_strict() {
        let mut source = CandidateSource {
            label: "Window + disorder".into(),
            family: "window_reorder".into(),
            urls: vec!["https://github.com/bol-van/zapret2/discussions/208".into()],
            evidence: SourceEvidence::AuthorReport,
            notes: "Source success is not a local result".into(),
        };
        assert!(source.valid());
        source.urls[0] = "https://github.com.evil.example/bol-van/recipe".into();
        assert!(!source.valid());
        source.urls[0] = "https://github.com/bol-van/zapret2/discussions/208".into();
        source.notes = "x".repeat(2049);
        assert!(!source.valid());
        assert!(serde_json::from_str::<DesyncStep>(
            r#"{"action":"fake","payload":["all"],"typo_range":"a0"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<CandidateSource>(
            r#"{"label":"x","family":"x","urls":[],"evidence":"local_success","notes":"x"}"#
        )
        .is_err());
    }
}
