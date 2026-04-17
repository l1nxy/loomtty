//! Validation for remote-host input (user@host[:ssh_port], host[:port], …).
//!
//! Every path that ultimately spawns `ssh` — the `ciritty remote` CLI flag,
//! the palette text-input prompt, `[[remote.hosts]]` entries from config, and
//! `recent_hosts.json` — must pass through one of these functions first.
//!
//! Specifically rejects:
//!   - leading '-' anywhere in the spec (the `-oProxyCommand=…` ssh
//!     argument-injection class, CVE-2017-1000117 and successors)
//!   - shell metacharacters in user or host (aligns with the OpenSSH 9.6
//!     cmdline block-list)
//!   - malformed or unbracketed IPv6 literals
//!   - port 0

use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

pub const MAX_USER: usize = 32;
pub const MAX_HOST: usize = 253;
pub const MAX_LABEL: usize = 63;
pub const MAX_TOTAL: usize = 320;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    Empty,
    TooLong,
    LeadingDash,
    UserEmpty,
    UserBadChar(char),
    UserTooLong,
    HostEmpty,
    HostBadChar(char),
    HostTooLong,
    LabelInvalid,
    UnbalancedBracket,
    InvalidIpv4,
    InvalidIpv6,
    NumericHostRequiresIpv4,
    InvalidPort(String),
    ReservedPortZero,
    TrailingColon,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ValidationError::*;
        match self {
            Empty => write!(f, "input is empty"),
            TooLong => write!(f, "input too long (max {MAX_TOTAL} chars)"),
            LeadingDash => write!(
                f,
                "input must not start with '-' (would be parsed as an ssh option)"
            ),
            UserEmpty => write!(f, "username is empty"),
            UserBadChar(c) => write!(f, "username contains invalid character {c:?}"),
            UserTooLong => write!(f, "username too long (max {MAX_USER} chars)"),
            HostEmpty => write!(f, "hostname is empty"),
            HostBadChar(c) => write!(f, "hostname contains invalid character {c:?}"),
            HostTooLong => write!(f, "hostname too long (max {MAX_HOST} chars)"),
            LabelInvalid => write!(
                f,
                "hostname label invalid (empty, too long, or boundary '-')"
            ),
            UnbalancedBracket => write!(f, "IPv6 literal missing or misplaced brackets"),
            InvalidIpv4 => write!(f, "invalid IPv4 address"),
            InvalidIpv6 => write!(f, "invalid IPv6 address"),
            NumericHostRequiresIpv4 => write!(
                f,
                "all-numeric hostnames must be a valid IPv4 address (use e.g. 1.2.3.4)"
            ),
            InvalidPort(s) => write!(f, "invalid port {s:?}"),
            ReservedPortZero => write!(f, "port 0 is reserved"),
            TrailingColon => write!(f, "input ends with ':' (missing port)"),
        }
    }
}

impl std::error::Error for ValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTarget {
    pub user: Option<String>,
    /// For IPv6 literals this keeps the surrounding `[...]` so it can be passed
    /// verbatim to `ssh` (which requires the brackets on the command line).
    pub host: String,
    pub ssh_port: u16,
}

/// Parse the palette text-input (`[user@]host[:ssh_port]`) and validate.
pub fn parse_input(input: &str, default_ssh_port: u16) -> Result<RemoteTarget, ValidationError> {
    let s = input.trim();
    check_outer(s)?;
    let (user, rest) = split_user(s)?;
    let (host, port) = split_host_port(rest, default_ssh_port)?;
    validate_host(&host)?;
    if port == 0 {
        return Err(ValidationError::ReservedPortZero);
    }
    Ok(RemoteTarget {
        user,
        host,
        ssh_port: port,
    })
}

/// Validate pre-split `host`, `remote_port`, `ssh_port` (CLI flags, config,
/// recent_hosts). `host_spec` may be `user@host` or bare `host`.
pub fn validate_fields(
    host_spec: &str,
    remote_port: u16,
    ssh_port: u16,
) -> Result<(), ValidationError> {
    let s = host_spec.trim();
    check_outer(s)?;
    let (_user, host_part) = split_user(s)?;
    validate_host(host_part)?;
    if remote_port == 0 || ssh_port == 0 {
        return Err(ValidationError::ReservedPortZero);
    }
    Ok(())
}

/// Split `[user@]rest`, validating the user portion if present. Returns
/// `(owned_user, borrowed_rest_of_input)`.
fn split_user(s: &str) -> Result<(Option<String>, &str), ValidationError> {
    match s.find('@') {
        Some(i) => {
            let u = &s[..i];
            validate_user(u)?;
            Ok((Some(u.to_string()), &s[i + 1..]))
        }
        None => Ok((None, s)),
    }
}

fn check_outer(s: &str) -> Result<(), ValidationError> {
    if s.is_empty() {
        return Err(ValidationError::Empty);
    }
    if s.len() > MAX_TOTAL {
        return Err(ValidationError::TooLong);
    }
    // Single most important rule: nothing in the spec may start with '-',
    // otherwise `ssh` parses it as an option flag regardless of position.
    if s.starts_with('-') {
        return Err(ValidationError::LeadingDash);
    }
    Ok(())
}

fn validate_user(u: &str) -> Result<(), ValidationError> {
    if u.is_empty() {
        return Err(ValidationError::UserEmpty);
    }
    if u.len() > MAX_USER {
        return Err(ValidationError::UserTooLong);
    }
    if u.starts_with('-') {
        return Err(ValidationError::UserBadChar('-'));
    }
    for c in u.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
        if !ok {
            return Err(ValidationError::UserBadChar(c));
        }
    }
    Ok(())
}

fn validate_host(h: &str) -> Result<(), ValidationError> {
    if h.is_empty() {
        return Err(ValidationError::HostEmpty);
    }
    if h.len() > MAX_HOST {
        return Err(ValidationError::HostTooLong);
    }

    // IPv6 literal: must be fully bracketed and parseable.
    if let Some(rest) = h.strip_prefix('[') {
        let Some(end) = rest.strip_suffix(']') else {
            return Err(ValidationError::UnbalancedBracket);
        };
        // RFC 6874: zone id suffix separated by '%'. Validate both halves —
        // the zone id still flows through `ssh` as part of the host arg and
        // ultimately into `sh -c` for the palette SSH-shell action, so
        // `[fe80::1%$(id)]` must be rejected even though the address half is
        // a valid IPv6 literal.
        let (addr_part, zone_part) = match end.split_once('%') {
            Some((a, z)) => (a, Some(z)),
            None => (end, None),
        };
        Ipv6Addr::from_str(addr_part).map_err(|_| ValidationError::InvalidIpv6)?;
        if let Some(zone) = zone_part {
            if zone.is_empty() {
                return Err(ValidationError::InvalidIpv6);
            }
            for c in zone.chars() {
                let ok = c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
                if !ok {
                    return Err(ValidationError::HostBadChar(c));
                }
            }
        }
        return Ok(());
    }
    if h.contains('[') || h.contains(']') {
        return Err(ValidationError::UnbalancedBracket);
    }

    // DNS / IPv4 / ssh-alias: RFC 1123 chars plus underscore. RFC 1123
    // doesn't allow '_', but `~/.ssh/config` Host aliases frequently use it
    // (`prod_db`, `stage_web`, …) and those flow through this validator on
    // the way to `ssh`. Rejecting them would break existing remote presets
    // without preventing any injection (underscore is not shell-active).
    for c in h.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
        if !ok {
            return Err(ValidationError::HostBadChar(c));
        }
    }
    if h.starts_with('.') {
        return Err(ValidationError::HostBadChar('.'));
    }
    let mut all_digits = true;
    for label in h.split('.') {
        if label.is_empty() || label.len() > MAX_LABEL {
            return Err(ValidationError::LabelInvalid);
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(ValidationError::LabelInvalid);
        }
        if !label.chars().all(|c| c.is_ascii_digit()) {
            all_digits = false;
        }
    }

    // If every label is pure digits, the whole host must be a valid IPv4
    // literal. Otherwise `1`, `1.2`, `999.999.999.999` etc. (almost always
    // typos) would pass RFC 1123 hostname syntax but then fail at DNS time
    // with an unhelpful error — catch them up front.
    if all_digits {
        if Ipv4Addr::from_str(h).is_err() {
            // Distinguish "looks like IPv4 but is malformed" from "bare int
            // that couldn't possibly be a hostname" for clearer messaging.
            return Err(if h.contains('.') {
                ValidationError::InvalidIpv4
            } else {
                ValidationError::NumericHostRequiresIpv4
            });
        }
    }
    Ok(())
}

/// Split `host`, `host:port`, `[v6]`, or `[v6]:port`. Returns the host spec
/// (brackets preserved for IPv6) and resolved port.
fn split_host_port(s: &str, default_port: u16) -> Result<(String, u16), ValidationError> {
    if let Some(stripped) = s.strip_prefix('[') {
        let Some(rb) = stripped.find(']') else {
            return Err(ValidationError::UnbalancedBracket);
        };
        let host = format!("[{}]", &stripped[..rb]);
        let after = &stripped[rb + 1..];
        if after.is_empty() {
            return Ok((host, default_port));
        }
        let Some(rest) = after.strip_prefix(':') else {
            let c = after.chars().next().unwrap();
            return Err(ValidationError::HostBadChar(c));
        };
        if rest.is_empty() {
            return Err(ValidationError::TrailingColon);
        }
        let port: u16 = rest
            .parse()
            .map_err(|_| ValidationError::InvalidPort(rest.to_string()))?;
        Ok((host, port))
    } else if let Some(colon) = s.rfind(':') {
        // Unbracketed IPv6 like `::1` or `fe80::1` is ambiguous with host:port;
        // require brackets (Q2 = strict).
        if s[..colon].contains(':') {
            return Err(ValidationError::UnbalancedBracket);
        }
        let host = s[..colon].to_string();
        let port_str = &s[colon + 1..];
        if port_str.is_empty() {
            return Err(ValidationError::TrailingColon);
        }
        let port: u16 = port_str
            .parse()
            .map_err(|_| ValidationError::InvalidPort(port_str.to_string()))?;
        Ok((host, port))
    } else {
        Ok((s.to_string(), default_port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(input: &str) -> RemoteTarget {
        parse_input(input, 22).unwrap_or_else(|e| panic!("expected ok for {input:?}, got {e}"))
    }
    fn parse_err(input: &str) -> ValidationError {
        parse_input(input, 22).unwrap_err()
    }

    // ── acceptance ────────────────────────────────────────────────
    #[test]
    fn accepts_user_at_host() {
        let t = parse_ok("user@example.com");
        assert_eq!(t.user.as_deref(), Some("user"));
        assert_eq!(t.host, "example.com");
        assert_eq!(t.ssh_port, 22);
    }

    #[test]
    fn accepts_bare_host() {
        let t = parse_ok("example.com");
        assert_eq!(t.user, None);
        assert_eq!(t.host, "example.com");
    }

    #[test]
    fn accepts_ipv4() {
        assert!(parse_input("root@192.168.1.1", 22).is_ok());
    }

    #[test]
    fn accepts_ipv6_bracketed() {
        let t = parse_ok("user@[::1]");
        assert_eq!(t.host, "[::1]");
        let t = parse_ok("user@[fe80::1%eth0]");
        assert_eq!(t.host, "[fe80::1%eth0]");
    }

    #[test]
    fn accepts_explicit_port() {
        assert_eq!(parse_ok("h.local:2222").ssh_port, 2222);
        assert_eq!(parse_ok("[::1]:2222").ssh_port, 2222);
    }

    #[test]
    fn accepts_usernames_with_legal_chars() {
        assert!(parse_input("a_b.c-d@host", 22).is_ok());
        assert!(parse_input("1user@host", 22).is_ok());
    }

    // ── the main reason we wrote this module ──────────────────────
    #[test]
    fn rejects_ssh_option_injection() {
        assert_eq!(parse_err("-oProxyCommand=evil"), ValidationError::LeadingDash);
        assert_eq!(
            parse_err("-oProxyCommand=foo@host"),
            ValidationError::LeadingDash
        );
        assert_eq!(parse_err("-lroot"), ValidationError::LeadingDash);
        // user starting with '-' must also fail
        assert!(matches!(
            parse_err("-bad@host"),
            ValidationError::LeadingDash | ValidationError::UserBadChar('-')
        ));
    }

    #[test]
    fn rejects_shell_metacharacters_in_host() {
        assert!(parse_input("user@host;rm", 22).is_err());
        assert!(parse_input("user@host|pwd", 22).is_err());
        assert!(parse_input("user@host$(whoami)", 22).is_err());
        assert!(parse_input("user@host name", 22).is_err());
        assert!(parse_input("user@host`id`", 22).is_err());
        assert!(parse_input("user@host\"quoted", 22).is_err());
    }

    #[test]
    fn rejects_shell_metacharacters_in_user() {
        assert!(parse_input("us er@host", 22).is_err());
        assert!(parse_input("u;r@host", 22).is_err());
        assert!(parse_input("u$@host", 22).is_err());
        assert!(parse_input("\"u\"@host", 22).is_err());
    }

    // ── structural ────────────────────────────────────────────────
    #[test]
    fn rejects_empty_and_degenerate() {
        assert_eq!(parse_err(""), ValidationError::Empty);
        assert_eq!(parse_err("   "), ValidationError::Empty);
        assert_eq!(parse_err("@host"), ValidationError::UserEmpty);
        assert_eq!(parse_err("user@"), ValidationError::HostEmpty);
        assert_eq!(parse_err("@"), ValidationError::UserEmpty);
    }

    #[test]
    fn rejects_host_boundary_chars() {
        assert!(matches!(
            parse_err("user@-bad"),
            ValidationError::LabelInvalid | ValidationError::HostBadChar(_)
        ));
        assert_eq!(parse_err("user@.bad"), ValidationError::HostBadChar('.'));
        assert!(matches!(parse_err("user@bad-"), ValidationError::LabelInvalid));
        assert!(matches!(parse_err("user@a..b"), ValidationError::LabelInvalid));
    }

    #[test]
    fn rejects_all_numeric_hostnames_unless_ipv4() {
        // Bare integer (the `ff@1` report) is almost certainly a typo.
        assert_eq!(
            parse_err("ff@1"),
            ValidationError::NumericHostRequiresIpv4
        );
        assert_eq!(parse_err("1"), ValidationError::NumericHostRequiresIpv4);
        assert_eq!(
            parse_err("999"),
            ValidationError::NumericHostRequiresIpv4
        );
        // Dotted-numeric that isn't a real IPv4.
        assert_eq!(parse_err("u@1.2"), ValidationError::InvalidIpv4);
        assert_eq!(parse_err("u@1.2.3"), ValidationError::InvalidIpv4);
        assert_eq!(
            parse_err("u@999.999.999.999"),
            ValidationError::InvalidIpv4
        );
        assert_eq!(
            parse_err("u@192.168.1.300"),
            ValidationError::InvalidIpv4
        );
        // Real IPv4 still accepted.
        assert!(parse_input("u@127.0.0.1", 22).is_ok());
        assert!(parse_input("u@10.0.0.1", 22).is_ok());
        // Any non-digit label means the all-digits rule doesn't trigger.
        assert!(parse_input("u@1a", 22).is_ok());
        assert!(parse_input("u@a1", 22).is_ok());
        assert!(parse_input("u@1.example.com", 22).is_ok());
    }

    #[test]
    fn rejects_shell_active_ipv6_zone_id() {
        // RFC 6874 zone ids slip past the IPv6 parser; check they can't
        // smuggle shell metacharacters into `ssh`.
        assert!(parse_input("u@[fe80::1%$(id)]", 22).is_err());
        assert!(parse_input("u@[fe80::1%`id`]", 22).is_err());
        assert!(parse_input("u@[fe80::1%;]", 22).is_err());
        assert!(parse_input("u@[fe80::1%]", 22).is_err()); // empty zone id
        // Legitimate zone ids still pass.
        assert!(parse_input("u@[fe80::1%eth0]", 22).is_ok());
        assert!(parse_input("u@[fe80::1%en_0]", 22).is_ok());
    }

    #[test]
    fn accepts_underscore_in_host_alias() {
        // ssh_config aliases routinely include `_`; we pass them through.
        assert!(parse_input("prod_db", 22).is_ok());
        assert!(parse_input("u@stage_web", 22).is_ok());
        assert!(parse_input("u@foo_bar.example.com", 22).is_ok());
    }

    #[test]
    fn rejects_ipv6_malformed() {
        assert!(matches!(
            parse_err("user@[bad"),
            ValidationError::UnbalancedBracket
        ));
        assert!(matches!(parse_err("user@::1"), ValidationError::UnbalancedBracket));
        assert_eq!(parse_err("user@[not-ipv6]"), ValidationError::InvalidIpv6);
    }

    #[test]
    fn rejects_port_zero_and_junk() {
        assert_eq!(parse_err("host:0"), ValidationError::ReservedPortZero);
        assert_eq!(parse_err("host:"), ValidationError::TrailingColon);
        assert!(matches!(parse_err("host:abc"), ValidationError::InvalidPort(_)));
        assert!(matches!(
            parse_err("host:99999"),
            ValidationError::InvalidPort(_)
        ));
    }

    #[test]
    fn rejects_overlong() {
        let long_user = "a".repeat(33);
        assert!(matches!(
            parse_input(&format!("{long_user}@host"), 22),
            Err(ValidationError::UserTooLong)
        ));

        let long_label = "a".repeat(64);
        assert!(matches!(
            parse_input(&format!("user@{long_label}.com"), 22),
            Err(ValidationError::LabelInvalid)
        ));

        let long_total = "a".repeat(MAX_TOTAL + 1);
        assert_eq!(
            parse_input(&long_total, 22).unwrap_err(),
            ValidationError::TooLong
        );
    }

    // ── validate_fields (non-palette entry points) ────────────────
    #[test]
    fn fields_accepts_bare_host() {
        assert!(validate_fields("example.com", 7890, 22).is_ok());
    }

    #[test]
    fn fields_rejects_leading_dash() {
        assert_eq!(
            validate_fields("-oProxyCommand=x", 7890, 22).unwrap_err(),
            ValidationError::LeadingDash
        );
    }

    #[test]
    fn fields_rejects_port_zero() {
        assert_eq!(
            validate_fields("h", 0, 22).unwrap_err(),
            ValidationError::ReservedPortZero
        );
        assert_eq!(
            validate_fields("h", 7890, 0).unwrap_err(),
            ValidationError::ReservedPortZero
        );
    }
}
