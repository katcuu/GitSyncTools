#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};

pub fn update_proxy() -> Option<String> {
    for name in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(value) = std::env::var(name) {
            if !value.trim().is_empty() {
                log::info!("operation=update_proxy source=environment variable={name}");
                return Some(value);
            }
        }
    }
    platform_proxy()
}

#[cfg(target_os = "macos")]
fn platform_proxy() -> Option<String> {
    let output = Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        log::warn!("operation=update_proxy source=macos result=scutil_failed");
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if let Some((kind, url)) = parse_macos_proxy(&text) {
        log::info!("operation=update_proxy source=macos type={kind}");
        return Some(url);
    }
    if text.contains("ProxyAutoConfigEnable : 1") {
        log::warn!("operation=update_proxy source=macos result=pac_only");
    } else {
        log::info!("operation=update_proxy source=macos result=not_configured");
    }
    None
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_proxy(text: &str) -> Option<(&'static str, String)> {
    for (prefix, scheme) in [("HTTPS", "http"), ("HTTP", "http"), ("SOCKS", "socks5h")] {
        if value(text, &format!("{prefix}Enable")) != Some("1") {
            continue;
        }
        let Some(host) = value(text, &format!("{prefix}Proxy")) else {
            continue;
        };
        let Some(port) = value(text, &format!("{prefix}Port")) else {
            continue;
        };
        if !host.is_empty() && port.parse::<u16>().is_ok() {
            return Some((prefix, format!("{scheme}://{host}:{port}")));
        }
    }
    None
}

#[cfg(any(target_os = "macos", test))]
fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.trim().split_once(" : ")?;
        (name == key).then_some(value.trim())
    })
}

#[cfg(not(target_os = "macos"))]
fn platform_proxy() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_https_proxy_from_scutil_output() {
        let output = r#"
<dictionary> {
  HTTPEnable : 0
  HTTPSEnable : 1
  HTTPSPort : 7890
  HTTPSProxy : 127.0.0.1
}"#;
        assert_eq!(
            parse_macos_proxy(output),
            Some(("HTTPS", "http://127.0.0.1:7890".into()))
        );
    }
}
