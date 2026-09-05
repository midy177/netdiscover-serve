# Netdiscover

English | [简体中文](README.zh-CN.md)

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Netdiscover is a CLI tool for discovering node network information — hostname,
underlay (private) IPv4 and public IPv4/IPv6. The typical use case is, when
running inside Kubernetes or a container, to discover the public IP and/or
hostname of the node the container is running on. This is commonly necessary
to configure VoIP applications.

This repository contains the **Rust (edition 2024)** implementation. It
replaces the original Go implementation and its per-cloud metadata providers
with a single unified discovery engine — STUN, UDP probing and HTTPS
fallbacks — while staying command-line and response compatible with the Go
CLI.

## Install

Requires a Rust toolchain (1.85+).

```sh
git clone https://github.com/<org>/<repo>.git
cd <repo>
cargo install --path .
```

Or build a release binary directly:

```sh
cargo build --release   # ./target/release/netdiscover-serve
```

## Quick start

```console
$ netdiscover-serve -field publicv4
203.0.113.7

$ netdiscover-serve -field privatev4
10.0.0.5

$ netdiscover-serve
{"hostname":"node1.example.com","private_ipv4":"10.0.0.5","public_ipv4":"203.0.113.7","public_ipv6":"","client_ip":"","client_port":0}

$ netdiscover-serve -debug
2026/09/03 12:14:51 underlay: default route device is en0
2026/09/03 12:14:51 underlay: UDP probe selected source address 10.10.148.41
{"hostname":"","private_ipv4":"10.10.148.41","public_ipv4":"203.0.113.7","public_ipv6":"","client_ip":"","client_port":0}

$ netdiscover-serve -serve -listen 0.0.0.0:8080
2026/09/03 12:14:51 server: TCP listening on 0.0.0.0:8080
2026/09/03 12:14:51 server: UDP listening on 0.0.0.0:8080
```

## Docker

```sh
docker run --rm -p 8080:8080/tcp -p 8080:8080/udp ghcr.io/<org>/<repo>:latest
```

The image starts `netdiscover-serve -serve` by default.

For a local image build, compile the Linux binary first, then copy it into the
runtime image:

```sh
cargo zigbuild --locked --release --target aarch64-unknown-linux-musl
mkdir -p dist/docker
cp target/aarch64-unknown-linux-musl/release/netdiscover-serve dist/docker/netdiscover-serve-arm64
docker build -t netdiscover-serve:test .
```

In service mode, hostname/private/public IP discovery runs once during startup.
When the request payload is `discover`, each TCP connection or UDP datagram
receives that cached response with `client_ip` and `client_port` filled from
the request peer address. Other non-empty payloads are echoed unchanged. Empty
UDP datagrams are ignored.

```sh
printf 'discover\n' | nc 127.0.0.1 8080
printf 'discover\n' | nc -u -w 1 127.0.0.1 8080
```

## CLI reference

```
Usage of netdiscover-serve:
  -debug
    	debug mode
  -field string
    	return only a single field.  Options are: "hostname", "publicv4", publicv6", "privatev4"
  -provider string
    	provider type.  Options are: "aws", "azure", "do", gcp"
  -serve
    	run TCP and UDP response service
  -listen string
    	listen address for -serve TCP and UDP service (default "0.0.0.0:8080")
  -tcp string
    	run TCP response service on this address
  -udp string
    	run UDP response service on this address
```

| Flag | Description |
|---|---|
| `-field <name>` | Return only a single field; omit (or use `""`) to get the full JSON response |
| `-debug` | Log individual discovery failures to stderr in Go `log` format instead of silently omitting fields |
| `-provider <name>` | Placeholder accepted for compatibility with the Go CLI; the value is discarded at parse time |
| `-serve` | Run both TCP and UDP services, defaulting to `0.0.0.0:8080` |
| `-listen <addr>` | Listen address used by `-serve` for both TCP and UDP |
| `-tcp <addr>` | Run only the TCP service on this address, or override TCP address when combined with `-serve` |
| `-udp <addr>` | Run only the UDP service on this address, or override UDP address when combined with `-serve` |
| `-h`, `-help` | Print usage and exit 0 |

Supported fields:

| Field | JSON key | Meaning |
|---|---|---|
| `hostname` | `hostname` | System hostname (`gethostname(2)`) |
| `privatev4` | `private_ipv4` | Underlay (private) IPv4 address of the node |
| `publicv4` | `public_ipv4` | Public (external) IPv4 address of the node |
| `publicv6` | `public_ipv6` | Public (external) IPv6 address of the node |
| n/a | `client_ip` | Request peer IP in service mode; empty in CLI mode |
| n/a | `client_port` | Request peer port in service mode; `0` in CLI mode |

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Success (including `-h`) |
| `1` | Discovery failed for a requested field, or invalid `-field` value |
| `2` | Command-line syntax error (Go-style message + usage) |

## Environment variables

| Variable | Effect |
|---|---|
| `UNDERLAY_IP` | Use this IPv4 directly as the underlay (private) IPv4 |
| `PUBLIC_IP` | Use this IPv4 directly as the public IPv4 |
| `STUN_SERVERS` | Comma-separated STUN servers (`host[:port]`, default port 3478); tried before the built-in list |
| `STUN_SERVER` | Fallback alias for `STUN_SERVERS` |
| `CLOUD_PROVIDER` | Accepted for Go-version compatibility; value is ignored |

## How discovery works

Each field is resolved through an ordered fallback chain; the first method
that succeeds wins.

**`privatev4` (underlay IP)**

1. `UNDERLAY_IP` environment variable
2. UDP probe — connect an unconnected UDP socket to a public target (8.8.8.8,
   1.1.1.1, 223.5.5.5, 9.9.9.9). No packet is sent: the kernel consults the
   routing table (the default route, i.e. the underlay device) and reports
   the source address it chose
3. First global-unicast IPv4 of the default-route device
   (`/proc/net/route` on Linux, `route -n get default` on macOS)
4. First global-unicast IPv4 of any non-`docker*`, non-loopback interface

**`publicv4` / `publicv6`**

1. `PUBLIC_IP` environment variable
2. STUN (RFC 5389 Binding) — servers from `STUN_SERVERS`/`STUN_SERVER` first,
   then a built-in list (stun.l.google.com:19302, stun.cloudflare.com:3478, …)
3. Plain-HTTPS endpoints — `https://api.ip.sb/ip`, `https://icanhazip.com`,
   `https://ifconfig.me/ip`, `https://api.ipify.org` (v4);
   `https://api6.ip.sb/ip`, `https://api64.ipify.org` (v6)

**`hostname`**

1. The system hostname, read directly via the `gethostname(2)` syscall
   (no subprocess, no DNS); a trailing dot is stripped

## Architecture

One-directional layering — `main` → `cli`/`output`/`log` → `discover` →
`system`; nothing in `main.rs` is referenced by lower layers.

```
src/
├── main.rs       entry point: flag dispatch + orchestration
├── cli.rs        Go `flag`-compatible argument parsing
├── log.rs        Go `log`-format output helpers
├── output.rs     Response structure + JSON encoding (byte-compatible)
├── discover/     discovery strategies
│   ├── underlay.rs   underlay IP fallback chain
│   ├── publicip.rs   public IP fallback chain (STUN → HTTPS)
│   ├── hostname.rs   system hostname via gethostname(2)
│   └── stun.rs       STUN policy over the `stunclient` crate (transport + RFC 5389 codec live in the library)
└── system/       OS interfaces
    ├── ifaddr.rs     interface enumeration (getifaddrs)
    └── route.rs      default-route detection (Linux/macOS, fixture-tested)
```

Extending: a new platform goes in `src/system/`; a new discovery precedence
goes in the strategy modules under `src/discover/`.

## Compatibility with the Go version

- Same flags, same usage text, same exit codes (flag errors exit 2; `-h`
  exits 0; invalid `-field` exits 1 with `valid fields are: …`)
- Single-field queries print the plain value + newline
- Full queries print one JSON line with all keys in a fixed order; failed
  discovery fields are empty strings unless `-debug` explains them

## Development

```sh
cargo build
cargo test               # offline unit tests; STUN/route parsers run against fixtures
cargo clippy --all-targets
cargo fmt --check
```

Note: public-IP discovery makes real STUN (UDP) and HTTPS calls; underlay
discovery only performs local routing-table lookups.

## License

Apache License 2.0 — see [LICENSE](LICENSE).
