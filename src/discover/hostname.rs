//! Public hostname discovery: reverse DNS of the public IPv4, with the same
//! filtering rules as the Go version's `defaultHostname`.

use std::ffi::CStr;
use std::net::{IpAddr, Ipv4Addr};

/// Resolve the public IPv4 to a public hostname.
///
/// `public` is the (possibly failed) public-IPv4 lookup, mirroring the Go
/// version, which derives the hostname from the public IP discovery function.
pub fn discover(public: &Result<Ipv4Addr, String>) -> Result<String, String> {
    let ip = public
        .as_ref()
        .map_err(|e| format!("failed to obtain public IP: {e}"))?;

    let names = reverse_lookup(IpAddr::V4(*ip))
        .map_err(|e| format!("failed to reverse-lookup ip address: {e}"))?;

    select_valid_name(&names).ok_or_else(|| "failed to discover valid public name".to_string())
}

/// Go-version filtering: skip implausibly short names, dot-less names and
/// `.local` names; return the first survivor without its trailing dot.
fn select_valid_name(names: &[String]) -> Option<String> {
    for name in names {
        if name.len() < 6 {
            continue; // implausibly short name
        }
        if !name.contains('.') {
            continue; // implausible TLD or local-only hostname
        }
        if name.ends_with(".local") {
            continue;
        }
        return Some(name.trim_end_matches('.').to_string());
    }
    None
}

/// Reverse DNS via getnameinfo(3) with NI_NAMEREQD, matching the semantics of
/// Go's net.LookupAddr for our purposes (return the PTR owner name).
fn reverse_lookup(ip: IpAddr) -> Result<Vec<String>, String> {
    let mut storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let len: libc::socklen_t = match ip {
        IpAddr::V4(v4) => {
            // Zero-initialized so that platform-specific fields such as the
            // BSD `sin_len` default to 0, which getnameinfo accepts.
            let mut sa: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            sa.sin_family = libc::AF_INET as libc::sa_family_t;
            sa.sin_addr = libc::in_addr {
                s_addr: u32::from(v4).to_be(),
            };
            // SAFETY: copying a sockaddr_in into a zeroed sockaddr_storage of
            // equal-or-larger size; both are plain C structs.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    &sa as *const libc::sockaddr_in as *const u8,
                    &mut storage as *mut libc::sockaddr_storage as *mut u8,
                    std::mem::size_of::<libc::sockaddr_in>(),
                );
            }
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t
        }
        IpAddr::V6(v6) => {
            let mut sa: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sa.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            sa.sin6_addr.s6_addr = v6.octets();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    &sa as *const libc::sockaddr_in6 as *const u8,
                    &mut storage as *mut libc::sockaddr_storage as *mut u8,
                    std::mem::size_of::<libc::sockaddr_in6>(),
                );
            }
            std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t
        }
    };

    // NI_MAXHOST
    let mut host = vec![0u8; 1025];
    // SAFETY: host buffer is owned by us and valid for writes of host.len().
    let res = unsafe {
        libc::getnameinfo(
            &storage as *const libc::sockaddr_storage as *const libc::sockaddr,
            len,
            host.as_mut_ptr() as *mut libc::c_char,
            host.len() as libc::socklen_t,
            std::ptr::null_mut(),
            0,
            libc::NI_NAMEREQD,
        )
    };
    if res != 0 {
        let detail = unsafe {
            let msg = libc::gai_strerror(res);
            if msg.is_null() {
                format!("error {res}")
            } else {
                CStr::from_ptr(msg).to_string_lossy().into_owned()
            }
        };
        return Err(format!("lookup {ip}: {detail}"));
    }

    let end = host.iter().position(|&b| b == 0).unwrap_or(host.len());
    let name = String::from_utf8_lossy(&host[..end]).into_owned();
    if name.is_empty() {
        return Err(format!("lookup {ip}: empty result"));
    }
    Ok(vec![name])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_filtering_matches_go() {
        let ip: Ipv4Addr = "203.0.113.7".parse().unwrap();

        // Propagation of a failed public-IP lookup.
        assert_eq!(
            discover(&Err("boom".to_string())).unwrap_err(),
            "failed to obtain public IP: boom"
        );
        let _ = ip;

        // Trailing dot is trimmed, valid names pass through.
        assert_eq!(
            select_valid_name(&["node1.example.com.".to_string()]),
            Some("node1.example.com".to_string())
        );
        // < 6 chars is implausibly short.
        assert_eq!(select_valid_name(&["ab.cd".to_string()]), None);
        // No dot: local-only hostname.
        assert_eq!(select_valid_name(&["localhost".to_string()]), None);
        // .local mDNS names are rejected.
        assert_eq!(select_valid_name(&["myhost.local".to_string()]), None);
        // First valid wins.
        assert_eq!(
            select_valid_name(&[
                "short".to_string(),
                "nodot".to_string(),
                "a.local".to_string(),
                "good.example.org".to_string()
            ]),
            Some("good.example.org".to_string())
        );
    }
}
