//! The client's address, for the login throttle. As Laravel's trusted-proxy setting
//! does, forwarded headers are believed only when the socket address belongs to a
//! proxy the app lists in `IGNITE_TRUSTED_PROXIES`. Any other client can send any
//! header, so its socket address is the only address the panel trusts.

use std::net::IpAddr;

/// The networks that may set forwarded headers. Read from `IGNITE_TRUSTED_PROXIES`,
/// a comma-separated list of IP addresses and CIDR ranges such as
/// `10.0.0.0/8, 192.0.2.10`. Empty when the variable is unset.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TrustedProxies {
    networks: Vec<Network>,
}

/// One address or CIDR range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Network {
    addr: IpAddr,
    prefix: u8,
}

impl TrustedProxies {
    /// Parses a list of addresses and ranges. An entry that does not parse is an error,
    /// so a typo stops the app at startup instead of trusting nothing silently.
    pub(crate) fn parse(list: &str) -> Result<Self, String> {
        let mut networks = Vec::new();
        for entry in list.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            networks.push(parse_network(entry)?);
        }
        Ok(TrustedProxies { networks })
    }

    /// Whether `ip` is one of the trusted proxies.
    pub(crate) fn contains(&self, ip: IpAddr) -> bool {
        let ip = canonical(ip);
        self.networks.iter().any(|network| network.contains(ip))
    }
}

fn parse_network(entry: &str) -> Result<Network, String> {
    let (addr_text, prefix_text) = match entry.split_once('/') {
        Some((addr, prefix)) => (addr, Some(prefix)),
        None => (entry, None),
    };
    let addr: IpAddr = addr_text
        .parse()
        .map_err(|_| format!("`{entry}` is not an IP address or a CIDR range"))?;
    let addr = canonical(addr);
    let largest = if addr.is_ipv4() { 32 } else { 128 };
    let prefix = match prefix_text {
        None => largest,
        Some(text) => text
            .parse::<u8>()
            .ok()
            .filter(|p| *p <= largest)
            .ok_or_else(|| format!("`{entry}` has a prefix length that is not 0 to {largest}"))?,
    };
    Ok(Network { addr, prefix })
}

impl Network {
    fn contains(&self, ip: IpAddr) -> bool {
        match (ip, self.addr) {
            (IpAddr::V4(ip), IpAddr::V4(net)) => {
                let mask = prefix_mask_u32(self.prefix);
                u32::from(ip) & mask == u32::from(net) & mask
            }
            (IpAddr::V6(ip), IpAddr::V6(net)) => {
                let mask = prefix_mask_u128(self.prefix);
                u128::from(ip) & mask == u128::from(net) & mask
            }
            _ => false,
        }
    }
}

fn prefix_mask_u32(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

fn prefix_mask_u128(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}

/// An IPv4-mapped IPv6 address (`::ffff:192.0.2.1`) is the IPv4 address, so both
/// spellings of a proxy match the same entry.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 @ IpAddr::V4(_) => v4,
    }
}

/// Whether the request came over HTTPS. `direct_tls` is true when every listener of the
/// server is TLS, so a request that reached it did so over HTTPS. Otherwise, when the
/// socket is a trusted proxy, the request is secure when the right-most `X-Forwarded-Proto`
/// entry is `https`. A forwarded scheme from any other socket is ignored.
pub(crate) fn request_is_secure(
    direct_tls: bool,
    socket: Option<IpAddr>,
    forwarded_proto: Option<&str>,
    trusted: &TrustedProxies,
) -> bool {
    if direct_tls {
        return true;
    }
    let Some(socket) = socket else {
        return false;
    };
    if !trusted.contains(socket) {
        return false;
    }
    forwarded_proto
        .and_then(|header| {
            header
                .split(',')
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .next_back()
        })
        .is_some_and(|proto| proto.eq_ignore_ascii_case("https"))
}

/// The client's address. `socket` is the address the connection came from, or `None`
/// when Rocket does not know it. When `socket` is not a trusted proxy, it is the
/// answer, and `forwarded_for` is ignored. When it is one, the answer is the
/// right-most `X-Forwarded-For` entry that is not a trusted proxy. The header is
/// added to by each proxy on the way, so the right-most entries are the ones the
/// trusted proxies wrote. A header that does not parse, or that lists only trusted
/// proxies, falls back to the socket address.
pub(crate) fn client_ip(
    socket: Option<IpAddr>,
    forwarded_for: Option<&str>,
    trusted: &TrustedProxies,
) -> Option<IpAddr> {
    let socket = canonical(socket?);
    if !trusted.contains(socket) {
        return Some(socket);
    }
    let Some(header) = forwarded_for else {
        return Some(socket);
    };
    let entries: Result<Vec<IpAddr>, _> = header
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| e.parse::<IpAddr>().map(canonical))
        .collect();
    let Ok(entries) = entries else {
        return Some(socket);
    };
    if let Some(client) = entries.iter().rev().find(|ip| !trusted.contains(**ip)) {
        return Some(*client);
    }
    Some(entries.first().copied().unwrap_or(socket))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn trusted(list: &str) -> TrustedProxies {
        TrustedProxies::parse(list).unwrap()
    }

    #[test]
    fn an_untrusted_socket_is_the_client_whatever_the_headers_say() {
        let none = TrustedProxies::default();
        assert_eq!(
            client_ip(Some(ip("198.51.100.4")), Some("203.0.113.9"), &none),
            Some(ip("198.51.100.4"))
        );
    }

    #[test]
    fn a_trusted_socket_takes_the_right_most_untrusted_forwarded_entry() {
        let proxies = trusted("192.0.2.10, 10.0.0.0/8");
        assert_eq!(
            client_ip(
                Some(ip("192.0.2.10")),
                Some("203.0.113.9, 10.1.2.3"),
                &proxies
            ),
            Some(ip("203.0.113.9"))
        );
        // A forged entry on the left is not believed: the proxy appended the right-most.
        assert_eq!(
            client_ip(
                Some(ip("192.0.2.10")),
                Some("1.1.1.1, 203.0.113.9"),
                &proxies
            ),
            Some(ip("203.0.113.9"))
        );
    }

    #[test]
    fn a_header_of_only_trusted_proxies_gives_the_left_most_entry() {
        let proxies = trusted("10.0.0.0/8");
        assert_eq!(
            client_ip(Some(ip("10.0.0.1")), Some("10.0.0.7, 10.0.0.8"), &proxies),
            Some(ip("10.0.0.7"))
        );
    }

    #[test]
    fn a_missing_or_broken_header_from_a_trusted_socket_is_the_socket() {
        let proxies = trusted("192.0.2.10");
        assert_eq!(
            client_ip(Some(ip("192.0.2.10")), None, &proxies),
            Some(ip("192.0.2.10"))
        );
        assert_eq!(
            client_ip(
                Some(ip("192.0.2.10")),
                Some("not-an-ip, 203.0.113.9"),
                &proxies
            ),
            Some(ip("192.0.2.10"))
        );
    }

    #[test]
    fn an_unknown_socket_has_no_address() {
        assert_eq!(
            client_ip(None, Some("203.0.113.9"), &trusted("10.0.0.0/8")),
            None
        );
    }

    #[test]
    fn ranges_and_single_addresses_match_by_prefix() {
        let proxies = trusted("10.0.0.0/8, 2001:db8::/32, 192.0.2.10");
        assert!(proxies.contains(ip("10.255.0.1")));
        assert!(!proxies.contains(ip("11.0.0.1")));
        assert!(proxies.contains(ip("2001:db8:1::5")));
        assert!(!proxies.contains(ip("2001:db9::5")));
        assert!(proxies.contains(ip("192.0.2.10")));
        assert!(!proxies.contains(ip("192.0.2.11")));
    }

    #[test]
    fn an_ipv4_mapped_address_matches_its_ipv4_entry() {
        let proxies = trusted("192.0.2.10");
        assert!(proxies.contains(ip("::ffff:192.0.2.10")));
        let mapped = trusted("::ffff:192.0.2.10");
        assert!(mapped.contains(ip("192.0.2.10")));
    }

    #[test]
    fn a_zero_prefix_trusts_its_whole_family() {
        let proxies = trusted("0.0.0.0/0");
        assert!(proxies.contains(ip("203.0.113.9")));
        assert!(!proxies.contains(ip("2001:db8::1")));
    }

    #[test]
    fn a_direct_tls_listener_is_secure_whatever_the_headers() {
        let none = TrustedProxies::default();
        assert!(request_is_secure(
            true,
            Some(ip("198.51.100.4")),
            None,
            &none
        ));
    }

    #[test]
    fn a_forwarded_scheme_from_an_untrusted_socket_is_ignored() {
        let none = TrustedProxies::default();
        assert!(!request_is_secure(
            false,
            Some(ip("198.51.100.4")),
            Some("https"),
            &none
        ));
    }

    #[test]
    fn a_trusted_proxy_decides_the_scheme_from_forwarded_proto() {
        let proxies = trusted("192.0.2.10");
        assert!(request_is_secure(
            false,
            Some(ip("192.0.2.10")),
            Some("https"),
            &proxies
        ));
        assert!(request_is_secure(
            false,
            Some(ip("192.0.2.10")),
            Some("HTTP, https"),
            &proxies
        ));
        assert!(!request_is_secure(
            false,
            Some(ip("192.0.2.10")),
            Some("http"),
            &proxies
        ));
        assert!(!request_is_secure(
            false,
            Some(ip("192.0.2.10")),
            None,
            &proxies
        ));
    }

    #[test]
    fn a_bad_entry_is_an_error() {
        assert!(TrustedProxies::parse("10.0.0.0/33").is_err());
        assert!(TrustedProxies::parse("example.com").is_err());
        assert!(TrustedProxies::parse("").unwrap() == TrustedProxies::default());
    }
}
