use std::fmt::Write;
use std::net::IpAddr;

use crate::constants::{ALWAYS_ALLOWED_HOSTS, TOKEN_BYTES};

/// The system could not supply random bytes for the desktop's access token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoRandomness;

/// ACCESS-02: 256 random bits from the system's secure random source, as hexadecimal.
pub fn generate_token() -> Result<String, NoRandomness> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| NoRandomness)?;
    Ok(bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    }))
}

/// ACCESS-04: compares in constant time, so the time taken reveals nothing about the token.
pub fn tokens_match(expected: &str, given: &str) -> bool {
    let expected = expected.as_bytes();
    let given = given.as_bytes();
    let difference = expected
        .iter()
        .zip(given)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        });
    expected.len() == given.len() && difference == 0
}

/// ACCESS-04: every data request carries the app's header. Other websites cannot add it
/// without the browser first asking this server, which never agrees, so they cannot send
/// requests here. The desktop app also requires its token in the header, because other
/// users of the same machine could reach its port.
pub fn request_is_allowed(token: Option<&str>, header: Option<&str>) -> bool {
    let Some(given) = header else {
        return false;
    };
    token.is_none_or(|expected| tokens_match(expected, given))
}

/// ACCESS-05: the allowed host names, always including `localhost` and `127.0.0.1`.
pub fn allowed_hosts(extra: &[String]) -> Vec<String> {
    ALWAYS_ALLOWED_HOSTS
        .iter()
        .map(|host| (*host).to_owned())
        .chain(extra.iter().map(|host| host.trim().to_ascii_lowercase()))
        .filter(|host| !host.is_empty())
        .collect()
}

/// ACCESS-05: the `Host` header must be an IP address or an allowed name; its port is
/// ignored. Names are limited because another website can point its own name at this
/// machine, while an address cannot be claimed that way.
pub fn host_is_allowed(host_header: &str, allowed: &[String]) -> bool {
    let host = host_without_port(host_header.trim()).to_ascii_lowercase();
    host.parse::<IpAddr>().is_ok() || allowed.contains(&host)
}

fn host_without_port(host: &str) -> &str {
    if let Some(bracketed) = host.strip_prefix('[') {
        return bracketed.split(']').next().unwrap_or_default();
    }
    host.rsplit_once(':').map_or(host, |(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ACCESS-02.
    #[test]
    fn tokens_are_long_random_and_different() {
        let first = generate_token().unwrap();
        let second = generate_token().unwrap();
        assert_eq!(first.len(), TOKEN_BYTES * 2);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    /// ACCESS-04.
    #[test]
    fn tokens_must_match_exactly() {
        assert!(tokens_match("abc123", "abc123"));
        assert!(!tokens_match("abc123", "abc124"));
        assert!(!tokens_match("abc123", "abc12"));
        assert!(!tokens_match("abc123", ""));
    }

    /// ACCESS-04: Docker needs only the header; the desktop needs its token in it.
    #[test]
    fn requests_need_the_header_and_on_the_desktop_the_token() {
        assert!(request_is_allowed(None, Some("")));
        assert!(!request_is_allowed(None, None));
        assert!(request_is_allowed(Some("abc123"), Some("abc123")));
        assert!(!request_is_allowed(Some("abc123"), Some("")));
        assert!(!request_is_allowed(Some("abc123"), None));
    }

    /// ACCESS-05.
    #[test]
    fn unexpected_host_is_rejected() {
        let allowed = allowed_hosts(&[String::from(" Subs.Local ")]);
        for host in [
            "127.0.0.1:41234",
            "localhost",
            "LOCALHOST:8080",
            "192.168.0.10:61880",
            "10.0.0.7",
            "[::1]:61880",
            "subs.local",
        ] {
            assert!(host_is_allowed(host, &allowed), "{host}");
        }
        for host in [
            "evil.example:61880",
            "127.0.0.1.evil.example",
            "192.168.0.10.nip.io",
            "",
        ] {
            assert!(!host_is_allowed(host, &allowed), "{host}");
        }
    }
}
