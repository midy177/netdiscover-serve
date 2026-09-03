//! Minimal STUN (RFC 5389) client used for public IP discovery.
//!
//! Only classic Binding requests over UDP are implemented — no external
//! dependencies, no channel binding, no credentials.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default STUN port applied when a server is configured without one.
pub const DEFAULT_STUN_PORT: u16 = 3478;

const MAGIC_COOKIE: [u8; 4] = [0x21, 0x12, 0xA4, 0x42];
const MSG_BINDING_SUCCESS: u16 = 0x0101;
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
/// Some older draft servers used 0x8028 for XOR-MAPPED-ADDRESS.
const ATTR_XOR_MAPPED_ADDRESS_DRAFT: u16 = 0x8028;

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(3);
const ATTEMPTS: usize = 2;
const MAX_SERVER_ADDRS: usize = 3;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Family {
    IPv4,
    IPv6,
}

/// Append the default STUN port when the input carries none.
pub fn normalize_server(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return s.to_string();
    }
    let colons = s.matches(':').count();
    if colons == 0 {
        format!("{s}:{DEFAULT_STUN_PORT}")
    } else if colons == 1 || s.starts_with('[') {
        s.to_string()
    } else {
        // Bare IPv6 literal.
        format!("[{s}]:{DEFAULT_STUN_PORT}")
    }
}

/// Query the servers in order; the first success wins.
pub fn discover(
    servers: &[String],
    family: Family,
    debug_log: impl Fn(&str),
) -> Result<IpAddr, String> {
    let mut errors: Vec<String> = Vec::new();
    for server in servers {
        debug_log(&format!("stun: trying {server}"));
        match query_server(server, family) {
            Ok(ip) => {
                debug_log(&format!("stun: {server} returned {ip}"));
                return Ok(ip);
            }
            Err(e) => errors.push(format!("{server}: {e}")),
        }
    }
    Err(format!("all STUN servers failed: {}", errors.join("; ")))
}

fn query_server(server: &str, family: Family) -> Result<IpAddr, String> {
    let addrs: Vec<SocketAddr> = server
        .to_socket_addrs()
        .map_err(|e| format!("failed to resolve: {e}"))?
        .filter(|a| match family {
            Family::IPv4 => a.is_ipv4(),
            Family::IPv6 => a.is_ipv6(),
        })
        .take(MAX_SERVER_ADDRS)
        .collect();
    if addrs.is_empty() {
        return Err("no matching address records".to_string());
    }
    let mut last = "no response".to_string();
    for addr in addrs {
        match query_addr(addr, family) {
            Ok(ip) => return Ok(ip),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn query_addr(addr: SocketAddr, family: Family) -> Result<IpAddr, String> {
    let bind_addr = match family {
        Family::IPv4 => "0.0.0.0:0",
        Family::IPv6 => "[::]:0",
    };
    let sock = UdpSocket::bind(bind_addr).map_err(|e| format!("bind failed: {e}"))?;
    sock.connect(addr)
        .map_err(|e| format!("connect failed: {e}"))?;

    let mut buf = [0u8; 2048];
    let mut last_err = "no response".to_string();

    for _ in 0..ATTEMPTS {
        let txid = transaction_id();
        let request = binding_request(&txid);
        sock.send(&request)
            .map_err(|e| format!("send failed: {e}"))?;

        let deadline = Instant::now() + ATTEMPT_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                last_err = "timed out".into();
                break;
            }
            sock.set_read_timeout(Some(remaining)).ok();
            match sock.recv(&mut buf) {
                Ok(n) => match parse_response(&buf[..n], &txid, family) {
                    Ok(ip) => return Ok(ip),
                    // Stray datagram for another transaction; keep waiting.
                    Err(ParseError::NotMatch) => continue,
                    Err(ParseError::Protocol(e)) => {
                        last_err = e;
                        break;
                    }
                },
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                    last_err = "timed out".into();
                    break;
                }
                Err(e) => {
                    last_err = format!("recv failed: {e}");
                    break;
                }
            }
        }
    }
    Err(last_err)
}

fn binding_request(txid: &[u8; 12]) -> [u8; 20] {
    let mut msg = [0u8; 20];
    msg[0..2].copy_from_slice(&0x0001u16.to_be_bytes()); // Binding Request
    msg[4..8].copy_from_slice(&MAGIC_COOKIE);
    msg[8..20].copy_from_slice(txid);
    msg
}

enum ParseError {
    /// Not addressed to our transaction: ignore and keep waiting.
    NotMatch,
    /// A definitive protocol failure in a response that was ours.
    Protocol(String),
}

/// Parse a Binding Success Response and return the mapped address.
fn parse_response(buf: &[u8], txid: &[u8; 12], want: Family) -> Result<IpAddr, ParseError> {
    if buf.len() < 20 {
        return Err(ParseError::Protocol("response too short".into()));
    }
    let msg_type = u16::from_be_bytes([buf[0], buf[1]]);
    let is_ours = buf[4..8] == MAGIC_COOKIE && buf[8..20] == txid[..];
    if msg_type != MSG_BINDING_SUCCESS {
        if is_ours {
            return Err(ParseError::Protocol(format!(
                "STUN message type 0x{msg_type:04x} is not a binding success"
            )));
        }
        return Err(ParseError::NotMatch);
    }
    if !is_ours {
        return Err(ParseError::NotMatch);
    }

    let msg_len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    let end = (20 + msg_len).min(buf.len());

    let mut xor_mapped: Option<IpAddr> = None;
    let mut mapped: Option<IpAddr> = None;
    let mut off = 20;
    while off + 4 <= end {
        let attr_type = u16::from_be_bytes([buf[off], buf[off + 1]]);
        let attr_len = u16::from_be_bytes([buf[off + 2], buf[off + 3]]) as usize;
        let vstart = off + 4;
        let vend = vstart + attr_len;
        if vend > end {
            break;
        }
        let value = &buf[vstart..vend];
        match attr_type {
            ATTR_XOR_MAPPED_ADDRESS | ATTR_XOR_MAPPED_ADDRESS_DRAFT => {
                if xor_mapped.is_none() {
                    xor_mapped = decode_address(value, Some(txid));
                }
            }
            ATTR_MAPPED_ADDRESS if mapped.is_none() => {
                mapped = decode_address(value, None);
            }
            _ => {}
        }
        off = vend.div_ceil(4) * 4; // attributes are padded to 32-bit boundaries
    }

    let ip = xor_mapped
        .or(mapped)
        .ok_or_else(|| ParseError::Protocol("no mapped address attribute".into()))?;

    match (want, ip) {
        (Family::IPv4, IpAddr::V4(_)) | (Family::IPv6, IpAddr::V6(_)) => Ok(ip),
        (want, ip) => Err(ParseError::Protocol(format!(
            "mapped address family does not match {want:?} query: {ip}"
        ))),
    }
}

/// Decode a (XOR-)MAPPED-ADDRESS attribute value into an IP address.
fn decode_address(v: &[u8], txid: Option<&[u8; 12]>) -> Option<IpAddr> {
    if v.len() < 4 {
        return None;
    }
    match v[1] {
        0x01 => {
            if v.len() < 8 {
                return None;
            }
            let raw = [v[4], v[5], v[6], v[7]];
            let octets = match txid {
                Some(_) => [
                    raw[0] ^ MAGIC_COOKIE[0],
                    raw[1] ^ MAGIC_COOKIE[1],
                    raw[2] ^ MAGIC_COOKIE[2],
                    raw[3] ^ MAGIC_COOKIE[3],
                ],
                None => raw,
            };
            Some(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        0x02 => {
            if v.len() < 20 {
                return None;
            }
            let mut key = [0u8; 16];
            key[..4].copy_from_slice(&MAGIC_COOKIE);
            if let Some(t) = txid {
                key[4..].copy_from_slice(t);
            }
            let mut octets = [0u8; 16];
            for (i, o) in octets.iter_mut().enumerate() {
                *o = v[4 + i] ^ key[i];
            }
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        _ => None,
    }
}

/// Best-effort unique transaction ID — STUN only requires uniqueness per
/// outstanding request; cryptographic randomness is not needed here.
fn transaction_id() -> [u8; 12] {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;

    let mut s0 = nanos ^ pid.rotate_left(17) ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut s1 = nanos.rotate_left(32) ^ (pid << 1) ^ (n << 32) | 1;
    let mut out = [0u8; 12];
    for b in out.iter_mut() {
        s0 ^= s0 << 13;
        s0 ^= s0 >> 7;
        s0 ^= s0 << 17;
        s1 ^= s1 << 25;
        s1 ^= s1 >> 12;
        s1 ^= s1 >> 27;
        *b =
            (s0.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ s1.wrapping_mul(0x9E37_79B9_7F4A_7C15)) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_response(txid: &[u8; 12], attr: u16, value: &[u8]) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&MSG_BINDING_SUCCESS.to_be_bytes());
        // Message length covers the attribute header (4) plus the value.
        msg.extend_from_slice(&((4 + value.len()) as u16).to_be_bytes());
        msg.extend_from_slice(&MAGIC_COOKIE);
        msg.extend_from_slice(txid);
        msg.extend_from_slice(&attr.to_be_bytes());
        msg.extend_from_slice(&(value.len() as u16).to_be_bytes());
        msg.extend_from_slice(value);
        msg
    }

    fn xor_ipv4_attr(ip: Ipv4Addr, port: u16) -> Vec<u8> {
        let mut value = vec![0u8, 0x01];
        value.extend_from_slice(&(port ^ 0x2112).to_be_bytes());
        for (o, k) in ip.octets().iter().zip(MAGIC_COOKIE.iter()) {
            value.push(o ^ k);
        }
        value
    }

    #[test]
    fn normalize_server_forms() {
        assert_eq!(
            normalize_server("stun.example.com"),
            "stun.example.com:3478"
        );
        assert_eq!(
            normalize_server("stun.example.com:19302"),
            "stun.example.com:19302"
        );
        assert_eq!(normalize_server("2001:db8::1"), "[2001:db8::1]:3478");
        assert_eq!(normalize_server("[2001:db8::1]:9999"), "[2001:db8::1]:9999");
        assert_eq!(normalize_server("  host  "), "host:3478");
    }

    #[test]
    fn parse_ipv4_xor_mapped() {
        let txid = [7u8; 12];
        let ip = Ipv4Addr::new(203, 0, 113, 7);
        let msg = build_response(&txid, ATTR_XOR_MAPPED_ADDRESS, &xor_ipv4_attr(ip, 47457));
        assert!(matches!(
            parse_response(&msg, &txid, Family::IPv4),
            Ok(IpAddr::V4(v)) if v == ip
        ));
        // Same payload under the old draft attribute code.
        let draft = build_response(
            &txid,
            ATTR_XOR_MAPPED_ADDRESS_DRAFT,
            &xor_ipv4_attr(ip, 47457),
        );
        assert!(matches!(
            parse_response(&draft, &txid, Family::IPv4),
            Ok(IpAddr::V4(v)) if v == ip
        ));
    }

    #[test]
    fn parse_ipv6_xor_mapped() {
        let txid = [9u8; 12];
        let ip = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1);
        let mut key = [0u8; 16];
        key[..4].copy_from_slice(&MAGIC_COOKIE);
        key[4..].copy_from_slice(&txid);

        let mut value = vec![0u8, 0x02];
        value.extend_from_slice(&(0x1234 ^ 0x2112u16).to_be_bytes());
        for (o, k) in ip.octets().iter().zip(key.iter()) {
            value.push(o ^ k);
        }

        let msg = build_response(&txid, ATTR_XOR_MAPPED_ADDRESS, &value);

        assert!(matches!(
            parse_response(&msg, &txid, Family::IPv6),
            Ok(IpAddr::V6(v)) if v == ip
        ));
        // An IPv4 query must reject the v6 result.
        assert!(parse_response(&msg, &txid, Family::IPv4).is_err());
    }

    #[test]
    fn parse_plain_mapped_address_fallback() {
        let txid = [1u8; 12];
        let ip = Ipv4Addr::new(198, 51, 100, 3);
        let mut value = vec![0u8, 0x01];
        value.extend_from_slice(&12345u16.to_be_bytes());
        value.extend_from_slice(&ip.octets());

        let msg = build_response(&txid, ATTR_MAPPED_ADDRESS, &value);

        assert!(matches!(
            parse_response(&msg, &txid, Family::IPv4),
            Ok(IpAddr::V4(v)) if v == ip
        ));
    }

    #[test]
    fn wrong_transaction_is_not_ours() {
        let msg = build_response(
            &[7u8; 12],
            ATTR_XOR_MAPPED_ADDRESS,
            &xor_ipv4_attr(Ipv4Addr::new(1, 2, 3, 4), 1),
        );
        assert!(matches!(
            parse_response(&msg, &[8u8; 12], Family::IPv4),
            Err(ParseError::NotMatch)
        ));
    }

    #[test]
    fn error_response_is_definitive() {
        let txid = [3u8; 12];
        let mut msg = Vec::new();
        msg.extend_from_slice(&0x0111u16.to_be_bytes()); // Binding error response
        msg.extend_from_slice(&0u16.to_be_bytes());
        msg.extend_from_slice(&MAGIC_COOKIE);
        msg.extend_from_slice(&txid);
        assert!(matches!(
            parse_response(&msg, &txid, Family::IPv4),
            Err(ParseError::Protocol(_))
        ));
    }
}
