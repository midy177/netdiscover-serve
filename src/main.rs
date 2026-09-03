//! netdiscover — Rust (edition 2024) rewrite of the Go original.
//!
//! Command-line arguments and JSON response structure are compatible with the
//! Go version (`-provider`, `-field`, `-debug`), but the per-cloud metadata
//! providers are replaced by a single unified discovery engine.
//!
//! Module layout:
//! * `cli` — Go `flag`-compatible argument parsing (`-provider` is a
//!   placeholder: accepted, never used)
//! * `log` — Go `log`-compatible output helpers
//! * `output` — response structure and JSON encoding
//! * `discover` — the discovery engine (underlay IP, public IP, hostname)
//! * `system` — OS interfaces (interface addresses, default route)

mod cli;
mod discover;
mod log;
mod output;
mod system;

use std::process::ExitCode;

fn main() -> ExitCode {
    let prog = std::env::args()
        .next()
        .unwrap_or_else(|| "netdiscover".into());
    let args: Vec<String> = std::env::args().skip(1).collect();

    let cfg = match cli::parse(&args) {
        Ok(c) => c,
        Err(cli::ParseError::Help) => {
            // Go's flag package prints usage for -h/-help and exits 0.
            eprint!("{}", cli::usage(&prog));
            return ExitCode::SUCCESS;
        }
        Err(cli::ParseError::Fail(msg)) => {
            // Go's failf prints the message followed by usage, exit code 2.
            eprintln!("{msg}");
            eprint!("{}", cli::usage(&prog));
            return ExitCode::from(2);
        }
    };

    let debug = cfg.debug;

    match cfg.field.as_deref().unwrap_or("") {
        "hostname" => {
            let public = discover::publicip::discover_v4(debug);
            match discover::hostname::discover(&public) {
                Ok(h) => {
                    println!("{h}");
                    ExitCode::SUCCESS
                }
                Err(e) => log::fatal(&e),
            }
        }
        "privatev4" => match discover::underlay::discover(debug) {
            Ok(ip) => {
                println!("{ip}");
                ExitCode::SUCCESS
            }
            Err(e) => log::fatal(&e),
        },
        "publicv4" => match discover::publicip::discover_v4(debug) {
            Ok(ip) => {
                println!("{ip}");
                ExitCode::SUCCESS
            }
            Err(e) => log::fatal(&e),
        },
        "publicv6" => match discover::publicip::discover_v6(debug) {
            Ok(ip) => {
                println!("{ip}");
                ExitCode::SUCCESS
            }
            Err(e) => log::fatal(&e),
        },
        "" => {
            let ret = discover_all(debug);
            println!("{}", output::encode_response(&ret));
            ExitCode::SUCCESS
        }
        _ => log::fatal("valid fields are: hostname, privatev4, publicv4, publicv6"),
    }
}

/// Run every discovery and collect the results; failed lookups stay empty,
/// like the Go version's default (non-debug) behaviour.
fn discover_all(debug: bool) -> output::Response {
    let mut ret = output::Response::default();

    let public = discover::publicip::discover_v4(debug);
    match discover::hostname::discover(&public) {
        Ok(h) => ret.hostname = h,
        Err(e) => log::debug_if(debug, &format!("failed to get hostname: {e}")),
    }
    match discover::underlay::discover(debug) {
        Ok(ip) => ret.private_ipv4 = ip.to_string(),
        Err(e) => log::debug_if(debug, &format!("failed to get private IPv4 address: {e}")),
    }
    match &public {
        Ok(ip) => ret.public_ipv4 = ip.to_string(),
        Err(e) => log::debug_if(debug, &format!("failed to get public IPv4 address: {e}")),
    }
    match discover::publicip::discover_v6(debug) {
        Ok(ip) => ret.public_ipv6 = ip.to_string(),
        Err(e) => log::debug_if(debug, &format!("failed to get public IPv6 address: {e}")),
    }

    ret
}
