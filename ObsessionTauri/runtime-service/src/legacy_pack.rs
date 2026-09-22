//! Compose verified, path-rewritten configs into one winws. Only capture ports
//! are unioned; L4/L7/host filters and desync strategies remain profile-local.
//! Input is render_legacy_config's one-option-per-line output, NOT raw config.
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Default)]
struct Ports(Vec<(u16, u16)>);

impl Ports {
    fn add(&mut self, value: &str) -> Result<()> {
        for part in value.split(',') {
            let (first, last) = part.split_once('-').unwrap_or((part, part));
            let parse = |text: &str| -> Result<u16> {
                if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("unsupported Legacy capture port syntax");
                }
                text.parse().map_err(|_| "invalid Legacy port")
            };
            let (first, last) = (parse(first)?, parse(last)?);
            if first == 0 || last < first {
                return Err("invalid Legacy port range");
            }
            self.0.push((first, last));
        }
        self.0.sort_unstable();
        let mut merged: Vec<(u16, u16)> = Vec::new();
        for &(first, last) in &self.0 {
            if let Some(previous) = merged.last_mut() {
                if u32::from(first) <= u32::from(previous.1) + 1 {
                    previous.1 = previous.1.max(last);
                    continue;
                }
            }
            merged.push((first, last));
        }
        self.0 = merged;
        Ok(())
    }

    fn includes(&self, value: &str) -> Result<bool> {
        let mut filter = Self::default();
        filter.add(value)?;
        Ok(filter
            .0
            .iter()
            .all(|&(first, last)| self.0.iter().any(|&(a, b)| a <= first && last <= b)))
    }

    fn render(&self) -> String {
        self.0
            .iter()
            .map(|&(first, last)| {
                if first == last {
                    first.to_string()
                } else {
                    format!("{first}-{last}")
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn option(line: &str) -> (&str, Option<&str>) {
    match line.split_once('=') {
        Some((key, value)) => (key, Some(value.trim_matches('"'))),
        None => (line, None),
    }
}

pub(crate) fn compile(configs: &[String]) -> Result<String> {
    if configs.is_empty() || configs.len() > 5 {
        return Err("invalid Legacy pack size");
    }
    let mut tcp = Ports::default();
    let mut udp = Ports::default();
    let mut strict = Vec::new();
    let mut learning = Vec::new();
    for config in configs {
        let mut local_tcp = Ports::default();
        let mut local_udp = Ports::default();
        let mut profiles: Vec<Vec<&str>> = vec![Vec::new()];
        for line in config.lines().filter(|line| !line.is_empty()) {
            let (key, value) = option(line);
            match key {
                "--wf-tcp" => {
                    let value = value.ok_or("missing TCP capture ports")?;
                    tcp.add(value)?;
                    local_tcp.add(value)?;
                }
                "--wf-udp" => {
                    let value = value.ok_or("missing UDP capture ports")?;
                    udp.add(value)?;
                    local_udp.add(value)?;
                }
                "--new" if value.is_none() => profiles.push(Vec::new()),
                key if key.starts_with("--wf-") => {
                    return Err("unsupported capture option in combined Legacy pack")
                }
                _ => profiles.last_mut().unwrap().push(line),
            }
        }
        let mut count = 0;
        for profile in profiles.into_iter().filter(|profile| !profile.is_empty()) {
            count += 1;
            let mut scoped = false;
            let mut auto = false;
            for line in &profile {
                let (key, value) = option(line);
                let capture = match key {
                    "--filter-tcp" => Some(&local_tcp),
                    "--filter-udp" => Some(&local_udp),
                    _ => None,
                };
                if let Some(capture) = capture {
                    scoped = true;
                    if !capture.includes(value.ok_or("missing profile port filter")?)? {
                        return Err("Legacy profile exceeds its original capture scope");
                    }
                }
                auto |= key == "--hostlist-auto";
            }
            if !scoped {
                return Err("combined Legacy profile requires explicit transport scope");
            }
            let original = profile.join("\n");
            if auto {
                // Learning also matches UNKNOWN hosts. Promote a strict copy
                // (including learned domains) ahead of all learning fallbacks,
                // otherwise Discord can swallow the YouTube profile.
                let known = profile
                    .iter()
                    .filter_map(|line| {
                        let (key, _) = option(line);
                        if key == "--hostlist-auto" {
                            Some(line.replacen("--hostlist-auto=", "--hostlist=", 1))
                        } else if key.starts_with("--hostlist-auto-") {
                            None
                        } else {
                            Some((*line).to_owned())
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                strict.push(known);
                learning.push(original);
            } else {
                strict.push(original);
            }
        }
        if count == 0 {
            return Err("Legacy config contains no profiles");
        }
    }
    let mut result = String::new();
    if !tcp.0.is_empty() {
        result.push_str(&format!("--wf-tcp=\"{}\"\n", tcp.render()));
    }
    if !udp.0.is_empty() {
        result.push_str(&format!("--wf-udp=\"{}\"\n", udp.render()));
    }
    strict.extend(learning);
    result.push_str(&strict.join("\n--new\n"));
    result.push('\n');
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_captures_become_one_union_and_profiles_stay_separate() {
        let discord = "--wf-tcp=\"443,2053\"\n--wf-udp=\"443,3478-3480\"\n--filter-tcp=\"443\"\n--hostlist=\"C:\\Program Files\\discord.txt\"\n--dpi-desync=\"multisplit\"\n--new\n";
        let youtube = "--wf-tcp=\"80,443\"\n--wf-udp=\"443\"\n--filter-tcp=\"443\"\n--hostlist=\"C:\\Program Files\\youtube.txt\"\n--dpi-desync=\"multidisorder\"\n";
        let result = compile(&[discord.into(), youtube.into()]).unwrap();
        assert_eq!(result.matches("--wf-tcp=").count(), 1);
        assert_eq!(result.matches("--wf-udp=").count(), 1);
        assert!(result.starts_with("--wf-tcp=\"80,443,2053\"\n--wf-udp=\"443,3478-3480\"\n"));
        assert!(result.contains("--dpi-desync=\"multisplit\"\n--new\n--filter-tcp=\"443\""));
        assert!(result.contains(r#"--hostlist="C:\Program Files\youtube.txt""#));
        assert_eq!(result.matches("--new").count(), 1);
    }

    #[test]
    fn learning_fallbacks_cannot_shadow_other_categories_known_hosts() {
        let a = "--wf-tcp=443\n--filter-tcp=443\n--hostlist=\"discord.txt\"\n--hostlist-auto=\"learned-discord.txt\"\n--hostlist-auto-fail-threshold=3\n--dpi-desync=fake\n";
        let b = "--wf-tcp=443\n--filter-tcp=443\n--hostlist=\"youtube.txt\"\n--dpi-desync=multidisorder\n";
        let result = compile(&[a.into(), b.into()]).unwrap();
        let profiles: Vec<_> = result.split("--new\n").collect();
        assert_eq!(profiles.len(), 3);
        assert!(profiles[0].contains("--hostlist=\"learned-discord.txt\""));
        assert!(!profiles[0].contains("--hostlist-auto"));
        assert!(profiles[1].contains("youtube.txt"));
        assert!(profiles[2].contains("--hostlist-auto=\"learned-discord.txt\""));
        assert!(profiles[2].contains("--hostlist-auto-fail-threshold=3"));
    }

    #[test]
    fn ports_are_bounded_normalized_and_never_widened() {
        let mut ports = Ports::default();
        ports.add("443,1024-65535,80,443,65535").unwrap();
        assert_eq!(ports.render(), "80,443,1024-65535");
        assert!(ports.includes("443,19294-19344").unwrap());
        assert!(!ports.includes("80-443").unwrap());
        for bad in ["", "0", "65536", "443-80", "~443", "1:5", "443,,80"] {
            assert!(Ports::default().add(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn unsupported_or_implicitly_broadened_profiles_fail_closed() {
        for bad in [
            "--wf-tcp=443\n--dpi-desync=fake\n",
            "--wf-tcp=443\n--filter-tcp=80,443\n--dpi-desync=fake\n",
            "--wf-tcp=443\n--filter-udp=443\n--dpi-desync=fake\n",
            "--wf-raw=custom\n--filter-tcp=443\n--dpi-desync=fake\n",
            "--wf-tcp=443\n--new\n",
        ] {
            assert!(compile(&[bad.into()]).is_err(), "{bad}");
        }
    }
}
