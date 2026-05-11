use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenSpec {
    Port(u16),
    Tcp { host: String, port: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseListenError {
    Empty,
    UnsupportedScheme(String),
    BadPort(String),
    Malformed(String),
}

impl fmt::Display for ParseListenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseListenError::Empty => write!(f, "listen spec must not be empty"),
            ParseListenError::UnsupportedScheme(s) => write!(
                f,
                "unsupported listen scheme `{s}:` (only bare port and `tcp://` are supported in this stage)"
            ),
            ParseListenError::BadPort(p) => {
                write!(f, "invalid port `{p}` in listen spec")
            }
            ParseListenError::Malformed(s) => {
                write!(f, "malformed listen spec `{s}`")
            }
        }
    }
}

impl std::error::Error for ParseListenError {}

impl ListenSpec {
    pub fn parse(raw: &str) -> Result<Self, ParseListenError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ParseListenError::Empty);
        }
        // Bare integer fast-path (mirrors reference cli.ts:106 `!isNaN(Number(...))`).
        if let Ok(p) = trimmed.parse::<u16>() {
            return Ok(ListenSpec::Port(p));
        }
        if let Some(rest) = trimmed.strip_prefix("tcp://") {
            let (host, port) = split_host_port(rest)?;
            // Q-001 closure: defaults mirror cli.ts:128-130.
            let host = if host.is_empty() {
                "localhost".to_string()
            } else {
                host
            };
            let port = port.unwrap_or(3000);
            return Ok(ListenSpec::Tcp { host, port });
        }
        if trimmed.starts_with("pipe:") {
            return Err(ParseListenError::UnsupportedScheme("pipe".to_string()));
        }
        if trimmed.starts_with("unix:") {
            return Err(ParseListenError::UnsupportedScheme("unix".to_string()));
        }
        Err(ParseListenError::Malformed(raw.to_string()))
    }

    pub fn resolve(&self) -> std::io::Result<SocketAddr> {
        match self {
            ListenSpec::Port(p) => Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), *p)),
            ListenSpec::Tcp { host, port } => {
                if let Ok(ip) = host.parse::<IpAddr>() {
                    return Ok(SocketAddr::new(ip, *port));
                }
                // Force "localhost" to IPv4 loopback so binding stays consistent
                // with the bare-port default. On dual-stack Windows hosts,
                // ToSocketAddrs may return ::1 first, which axum's listener can
                // bind v6-only and reject v4 clients.
                if host.eq_ignore_ascii_case("localhost") {
                    return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), *port));
                }
                (host.as_str(), *port)
                    .to_socket_addrs()?
                    .next()
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::AddrNotAvailable,
                            format!("could not resolve host {host:?}"),
                        )
                    })
            }
        }
    }
}

fn split_host_port(rest: &str) -> Result<(String, Option<u16>), ParseListenError> {
    if let Some(after_bracket) = rest.strip_prefix('[') {
        let end = after_bracket
            .find(']')
            .ok_or_else(|| ParseListenError::Malformed(rest.to_string()))?;
        let host = after_bracket[..end].to_string();
        let after = &after_bracket[end + 1..];
        if after.is_empty() {
            return Ok((host, None));
        }
        let port_str = after
            .strip_prefix(':')
            .ok_or_else(|| ParseListenError::Malformed(rest.to_string()))?;
        if port_str.contains('/') {
            return Err(ParseListenError::Malformed(rest.to_string()));
        }
        let port = port_str
            .parse::<u16>()
            .map_err(|_| ParseListenError::BadPort(port_str.to_string()))?;
        return Ok((host, Some(port)));
    }
    if rest.contains('/') {
        return Err(ParseListenError::Malformed(rest.to_string()));
    }
    if let Some((host, port_str)) = rest.rsplit_once(':') {
        if port_str.is_empty() {
            return Ok((host.to_string(), None));
        }
        let port = port_str
            .parse::<u16>()
            .map_err(|_| ParseListenError::BadPort(port_str.to_string()))?;
        Ok((host.to_string(), Some(port)))
    } else {
        Ok((rest.to_string(), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_port() {
        assert_eq!(ListenSpec::parse("3010"), Ok(ListenSpec::Port(3010)));
        assert_eq!(ListenSpec::parse("0"), Ok(ListenSpec::Port(0)));
    }

    #[test]
    fn parses_tcp_full() {
        assert_eq!(
            ListenSpec::parse("tcp://127.0.0.1:3010"),
            Ok(ListenSpec::Tcp {
                host: "127.0.0.1".into(),
                port: 3010,
            })
        );
    }

    #[test]
    fn parses_tcp_default_port_q001() {
        assert_eq!(
            ListenSpec::parse("tcp://localhost"),
            Ok(ListenSpec::Tcp {
                host: "localhost".into(),
                port: 3000,
            })
        );
    }

    #[test]
    fn parses_tcp_default_host_q001() {
        assert_eq!(
            ListenSpec::parse("tcp://:3010"),
            Ok(ListenSpec::Tcp {
                host: "localhost".into(),
                port: 3010,
            })
        );
    }

    #[test]
    fn parses_tcp_both_defaults_q001() {
        assert_eq!(
            ListenSpec::parse("tcp://"),
            Ok(ListenSpec::Tcp {
                host: "localhost".into(),
                port: 3000,
            })
        );
    }

    #[test]
    fn parses_ipv6_bracketed_with_port() {
        assert_eq!(
            ListenSpec::parse("tcp://[::1]:3010"),
            Ok(ListenSpec::Tcp {
                host: "::1".into(),
                port: 3010,
            })
        );
    }

    #[test]
    fn parses_ipv6_bracketed_default_port() {
        assert_eq!(
            ListenSpec::parse("tcp://[::1]"),
            Ok(ListenSpec::Tcp {
                host: "::1".into(),
                port: 3000,
            })
        );
    }

    #[test]
    fn rejects_empty_input() {
        assert_eq!(ListenSpec::parse(""), Err(ParseListenError::Empty));
        assert_eq!(ListenSpec::parse("   "), Err(ParseListenError::Empty));
    }

    #[test]
    fn rejects_bad_port() {
        assert!(matches!(
            ListenSpec::parse("tcp://host:notnum"),
            Err(ParseListenError::BadPort(_))
        ));
        assert!(matches!(
            ListenSpec::parse("tcp://[::1]:notnum"),
            Err(ParseListenError::BadPort(_))
        ));
    }

    #[test]
    fn rejects_pipe_scheme() {
        assert!(matches!(
            ListenSpec::parse(r"pipe:\\.\pipe\foo"),
            Err(ParseListenError::UnsupportedScheme(s)) if s == "pipe"
        ));
    }

    #[test]
    fn rejects_unix_scheme() {
        assert!(matches!(
            ListenSpec::parse("unix:/tmp/sock"),
            Err(ParseListenError::UnsupportedScheme(s)) if s == "unix"
        ));
    }

    #[test]
    fn rejects_path_component() {
        assert!(matches!(
            ListenSpec::parse("tcp://host:3010/path"),
            Err(ParseListenError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_unbalanced_bracket() {
        assert!(matches!(
            ListenSpec::parse("tcp://[::1"),
            Err(ParseListenError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_unknown_form() {
        assert!(matches!(
            ListenSpec::parse("garbage"),
            Err(ParseListenError::Malformed(_))
        ));
    }

    #[test]
    fn resolves_bare_port_to_v4_loopback() {
        let s = ListenSpec::Port(3010);
        assert_eq!(
            s.resolve().unwrap(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3010)
        );
    }

    #[test]
    fn resolves_localhost_to_v4_loopback() {
        let s = ListenSpec::Tcp {
            host: "localhost".into(),
            port: 3010,
        };
        assert_eq!(
            s.resolve().unwrap(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3010)
        );
    }

    #[test]
    fn resolves_v4_literal() {
        let s = ListenSpec::Tcp {
            host: "127.0.0.1".into(),
            port: 3010,
        };
        assert_eq!(
            s.resolve().unwrap(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3010)
        );
    }

    #[test]
    fn resolves_v6_literal() {
        let s = ListenSpec::Tcp {
            host: "::1".into(),
            port: 3010,
        };
        let resolved = s.resolve().unwrap();
        assert!(resolved.ip().is_loopback());
        assert!(resolved.is_ipv6());
        assert_eq!(resolved.port(), 3010);
    }
}
