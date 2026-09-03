//! Command-line parsing compatible with the Go `flag` package:
//! `-flag value`, `-flag=value`, `--flag value`, `--flag=value`; boolean flags
//! accept `-flag` or `-flag=true` (never a space-separated value); parsing
//! stops at the first positional argument or a bare `--`.

/// Parsed command-line configuration.
///
/// `provider` is intentionally absent: the flag is accepted as a placeholder
/// for Go-version compatibility, but its value is discarded at parse time.
#[derive(Default)]
pub struct Config {
    pub debug: bool,
    pub field: Option<String>,
}

#[derive(Debug)]
pub enum ParseError {
    /// `-h` / `-help`: print usage and exit 0 (Go flag ErrHelp behaviour).
    Help,
    /// Print the message followed by usage to stderr; exit code 2.
    Fail(String),
}

/// Usage text in the exact layout of Go's `flag.PrintDefaults`.
pub fn usage(prog: &str) -> String {
    let mut s = format!("Usage of {prog}:\n");
    s.push_str("  -debug\n");
    s.push_str("    \tdebug mode\n");
    s.push_str("  -field string\n");
    s.push_str(
        "    \treturn only a single field.  Options are: \"hostname\", \
         \"publicv4\", publicv6\", \"privatev4\"\n",
    );
    s.push_str("  -provider string\n");
    s.push_str("    \tprovider type.  Options are: \"aws\", \"azure\", \"do\", gcp\"\n");
    s
}

/// Parse arguments the way Go's `flag` package does.
pub fn parse(args: &[String]) -> Result<Config, ParseError> {
    let mut cfg = Config::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        // A non-flag argument terminates parsing (Go: flag.Parse stops there).
        if !arg.starts_with('-') || arg.len() < 2 {
            break;
        }

        let mut num_minuses = 1;
        if arg.starts_with("--") {
            if arg.len() == 2 {
                break; // bare "--" terminates flag parsing
            }
            num_minuses = 2;
        }

        let body = &arg[num_minuses..];
        if body.is_empty() || body.starts_with('-') || body.starts_with('=') {
            return Err(ParseError::Fail(format!("bad flag syntax: {arg}")));
        }

        let (name, value, has_value) = match body.find('=') {
            Some(p) => (&body[..p], body[p + 1..].to_string(), true),
            None => (body, String::new(), false),
        };

        match name {
            "debug" => {
                if has_value {
                    match value.parse::<bool>() {
                        Ok(v) => cfg.debug = v,
                        Err(_) => {
                            return Err(ParseError::Fail(format!(
                                "invalid boolean value \"{value}\" for -debug: parse error"
                            )));
                        }
                    }
                } else {
                    cfg.debug = true;
                }
            }
            "provider" => {
                // Placeholder: accepted for compatibility, value ignored.
                if !has_value {
                    i += 1;
                    if i >= args.len() {
                        return Err(ParseError::Fail(format!("flag needs an argument: -{name}")));
                    }
                }
            }
            "field" => {
                let value = if has_value {
                    value
                } else {
                    i += 1;
                    if i >= args.len() {
                        return Err(ParseError::Fail(format!("flag needs an argument: -{name}")));
                    }
                    args[i].clone()
                };
                if name == "field" {
                    cfg.field = Some(value);
                }
            }
            "h" | "help" => return Err(ParseError::Help),
            _ => {
                return Err(ParseError::Fail(format!(
                    "flag provided but not defined: -{name}"
                )));
            }
        }

        i += 1;
    }

    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn go_style_flag_forms() {
        // `-provider` is a placeholder: accepted (with its value) but ignored.
        let c = parse(&args(&["-provider", "gcp", "-field", "publicv4", "-debug"])).unwrap();
        assert!(c.debug);
        assert_eq!(c.field.as_deref(), Some("publicv4"));

        let c = parse(&args(&["--field=hostname"])).unwrap();
        assert_eq!(c.field.as_deref(), Some("hostname"));
        assert!(!c.debug);

        let c = parse(&args(&["-debug=false"])).unwrap();
        assert!(!c.debug);

        // `-field ""` behaves like the Go version: empty value, flag present.
        let c = parse(&args(&["-field", ""])).unwrap();
        assert_eq!(c.field.as_deref(), Some(""));
    }

    #[test]
    fn provider_is_placeholder() {
        // Any provider value parses fine and is simply discarded.
        assert!(parse(&args(&["-provider", "aws"])).is_ok());
        assert!(parse(&args(&["-provider=gcp"])).is_ok());
        assert!(parse(&args(&["--provider", "do"])).is_ok());
    }

    #[test]
    fn parsing_stops_at_positional() {
        // Everything from the first positional onward is ignored, like Go.
        let c = parse(&args(&["-debug", "extra", "-field", "publicv4"])).unwrap();
        assert!(c.debug);
        assert_eq!(c.field, None);

        let c = parse(&args(&["-debug", "--", "-field", "publicv4"])).unwrap();
        assert!(c.debug);
        assert_eq!(c.field, None);
    }

    #[test]
    fn error_messages_match_go() {
        assert!(matches!(
            parse(&args(&["-nope"])),
            Err(ParseError::Fail(m)) if m == "flag provided but not defined: -nope"
        ));
        assert!(matches!(
            parse(&args(&["-provider"])),
            Err(ParseError::Fail(m)) if m == "flag needs an argument: -provider"
        ));
        assert!(matches!(
            parse(&args(&["-debug=x"])),
            Err(ParseError::Fail(m))
                if m == "invalid boolean value \"x\" for -debug: parse error"
        ));
        assert!(matches!(parse(&args(&["-h"])), Err(ParseError::Help)));
        assert!(matches!(parse(&args(&["-help"])), Err(ParseError::Help)));
        assert!(matches!(
            parse(&args(&["---x"])),
            Err(ParseError::Fail(m)) if m == "bad flag syntax: ---x"
        ));
        assert!(matches!(
            parse(&args(&["-=v"])),
            Err(ParseError::Fail(m)) if m == "bad flag syntax: -=v"
        ));
    }
}
