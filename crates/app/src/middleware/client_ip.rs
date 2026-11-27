//! Client address detection for requests that may pass a reverse proxy.
//!
//! Forwarding headers such as `X-Real-IP` are set by whoever sends the
//! request. They are only honoured when the TCP peer is a trusted proxy:
//! loopback addresses, and the networks passed with `--trusted-proxy`.
//! For every other peer the socket address is the client address.

use std::net::{IpAddr, SocketAddr};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Request, Response},
    middleware::Next,
};
use real::RealIp;

use crate::ARGS;

const FORWARDED_FOR: &str = "x-forwarded-for";
const REAL_IP: &str = "x-real-ip";

/// An address or CIDR network of a reverse proxy whose headers are trusted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrustedProxy {
    network: IpAddr,
    prefix: u8,
}

impl TrustedProxy {
    /// Parse `IP` or `IP/PREFIX`, for example `10.0.0.5` or `172.16.0.0/12`.
    pub fn parse(value: &str) -> Result<Self, String> {
        let invalid = || format!("Invalid trusted proxy {value:?}; expected IP or IP/PREFIX");
        let (address, prefix) = match value.trim().split_once('/') {
            Some((address, prefix)) => (address, Some(prefix)),
            None => (value.trim(), None),
        };
        let network: IpAddr = address.parse().map_err(|_| invalid())?;
        let max_prefix = match network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        let prefix = match prefix {
            Some(prefix) => prefix.parse::<u8>().map_err(|_| invalid())?,
            None => max_prefix,
        };

        if prefix > max_prefix {
            return Err(invalid());
        }

        Ok(Self { network, prefix })
    }

    fn contains(&self, address: IpAddr) -> bool {
        match (self.network, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);

                u32::from(network) & mask == u32::from(address) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);

                u128::from(network) & mask == u128::from(address) & mask
            }
            _ => false,
        }
    }
}

/// IPv4-mapped IPv6 peers of a dual-stack listener are compared as IPv4.
fn canonical(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 @ IpAddr::V4(_) => v4,
    }
}

fn is_trusted(address: IpAddr, trusted: &[TrustedProxy]) -> bool {
    address.is_loopback() || trusted.iter().any(|proxy| proxy.contains(address))
}

fn header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    let value = headers.get(name)?.to_str().ok()?;

    value.trim().parse().ok().map(canonical)
}

/// Resolve the client address of a request received from `peer`.
///
/// `X-Forwarded-For` is read from the right: every proxy appends the address
/// it received the request from, so the first entry that is not a trusted
/// proxy is the client. Entries left of it are client-supplied.
pub fn resolve_client_ip(peer: IpAddr, headers: &HeaderMap, trusted: &[TrustedProxy]) -> IpAddr {
    let peer = canonical(peer);

    if !is_trusted(peer, trusted) {
        return peer;
    }

    let forwarded = headers
        .get_all(FORWARDED_FOR)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|entry| entry.trim().parse::<IpAddr>().map(canonical))
        .collect::<Vec<_>>();

    for entry in forwarded.iter().rev() {
        match entry {
            Ok(address) if is_trusted(*address, trusted) => {}
            Ok(address) => return *address,
            // Do not skip over a malformed hop to an attacker-chosen entry.
            Err(_) => return peer,
        }
    }

    if forwarded.is_empty()
        && let Some(address) = header_ip(headers, REAL_IP)
    {
        return address;
    }

    // Every hop is a trusted proxy: the request originates from the leftmost.
    forwarded
        .first()
        .and_then(|entry| entry.as_ref().ok().copied())
        .unwrap_or(peer)
}

/// Store the resolved client address as [`RealIp`] for handlers and middleware.
pub async fn client_ip(mut req: Request<Body>, next: Next) -> Response<Body> {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip());

    if let Some(peer) = peer {
        let ip = resolve_client_ip(peer, req.headers(), &ARGS.trusted_proxy);

        req.extensions_mut().insert(RealIp(ip));
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(entries: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();

        for (name, value) in entries {
            map.append(*name, value.parse().unwrap());
        }

        map
    }

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    #[test]
    fn direct_clients_cannot_choose_their_address() {
        let spoofed = headers(&[
            ("x-real-ip", "198.51.100.7"),
            ("x-forwarded-for", "198.51.100.8"),
        ]);

        assert_eq!(
            resolve_client_ip(ip("203.0.113.9"), &spoofed, &[]),
            ip("203.0.113.9")
        );
        assert_eq!(
            resolve_client_ip(ip("192.168.1.20"), &spoofed, &[]),
            ip("192.168.1.20")
        );
    }

    #[test]
    fn loopback_proxy_headers_are_used() {
        let real_ip = headers(&[("x-real-ip", "198.51.100.7")]);
        let forwarded = headers(&[("x-forwarded-for", "198.51.100.8")]);

        assert_eq!(
            resolve_client_ip(ip("127.0.0.1"), &real_ip, &[]),
            ip("198.51.100.7")
        );
        assert_eq!(
            resolve_client_ip(ip("::1"), &forwarded, &[]),
            ip("198.51.100.8")
        );
        assert_eq!(
            resolve_client_ip(ip("127.0.0.1"), &HeaderMap::new(), &[]),
            ip("127.0.0.1")
        );
    }

    #[test]
    fn forwarded_for_uses_the_entry_added_by_the_trusted_proxy() {
        let trusted = [TrustedProxy::parse("10.0.0.0/8").unwrap()];
        let spoofed = headers(&[("x-forwarded-for", "1.2.3.4, 198.51.100.8, 10.0.0.2")]);
        let malformed = headers(&[("x-forwarded-for", "1.2.3.4, unknown")]);

        assert_eq!(
            resolve_client_ip(ip("10.1.2.3"), &spoofed, &trusted),
            ip("198.51.100.8")
        );
        assert_eq!(
            resolve_client_ip(ip("10.1.2.3"), &malformed, &trusted),
            ip("10.1.2.3")
        );
    }

    #[test]
    fn trusted_proxy_parsing_and_matching() {
        let single = TrustedProxy::parse("192.168.1.5").unwrap();
        let network = TrustedProxy::parse("172.16.0.0/12").unwrap();
        let v6 = TrustedProxy::parse("fd00::/8").unwrap();
        let all = TrustedProxy::parse("0.0.0.0/0").unwrap();

        assert!(single.contains(ip("192.168.1.5")));
        assert!(!single.contains(ip("192.168.1.6")));
        assert!(network.contains(ip("172.31.255.1")));
        assert!(!network.contains(ip("172.32.0.1")));
        assert!(v6.contains(ip("fd12::1")));
        assert!(!v6.contains(ip("fe80::1")));
        assert!(!v6.contains(ip("10.0.0.1")));
        assert!(all.contains(ip("8.8.8.8")));
        assert!(TrustedProxy::parse("10.0.0.0/33").is_err());
        assert!(TrustedProxy::parse("proxy.local").is_err());
    }

    #[test]
    fn mapped_ipv4_peers_match_ipv4_rules() {
        let trusted = [TrustedProxy::parse("10.0.0.0/8").unwrap()];
        let forwarded = headers(&[("x-forwarded-for", "198.51.100.8")]);

        assert_eq!(
            resolve_client_ip(ip("::ffff:10.0.0.1"), &forwarded, &trusted),
            ip("198.51.100.8")
        );
    }
}
