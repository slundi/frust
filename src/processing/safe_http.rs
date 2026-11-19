//! Hardened HTTP fetching for URLs taken from feed content (Force-mode
//! article pages, later media assets).
//!
//! A compromised or hostile feed controls those URLs, and on a router the
//! obvious target is the LAN admin interface. Every hop is therefore checked:
//!
//! * the URL itself: scheme, local-only domain names, IP literals;
//! * every redirect target, with the same rules;
//! * every address a domain resolves to. The check runs inside the client's
//!   resolver, so the connection uses exactly the vetted addresses and a DNS
//!   rebinding between "check" and "connect" is impossible;
//! * the response: 2xx only, HTML content type, bounded body.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use reqwest::{
    Client, Url,
    dns::{Addrs, Name, Resolve, Resolving},
    header, redirect,
};

use super::read_bounded_body;
use crate::error::FrustError;

/// Cap on a scraped page body. Article pages rarely exceed a few hundred KB;
/// reading unbounded would OOM the router-class targets.
pub(crate) const MAX_PAGE_BYTES: u64 = 8 * 1024 * 1024; // 8 MiB
const MAX_REDIRECTS: usize = 5;

/// Domain suffixes that only resolve on a local network: RFC 6761/6762
/// (`localhost`, `local`), RFC 8375 (`home.arpa`), OpenWrt's default `lan`,
/// and common home / corporate conventions.
const LOCAL_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "internal",
    "lan",
    "home.arpa",
    "home",
    "corp",
    "intranet",
    "private",
];

/// Which targets a client may reach.
#[derive(Debug, Clone, Copy)]
struct Reach {
    /// Only set by tests, so they can talk to a server on 127.0.0.1.
    allow_loopback: bool,
}

impl Reach {
    const PUBLIC: Self = Self {
        allow_loopback: false,
    };

    fn allows_ip(self, ip: IpAddr) -> bool {
        is_public_ip(ip) || (self.allow_loopback && ip.is_loopback())
    }

    fn allows_url(self, url: &Url) -> bool {
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        match url.host() {
            Some(url::Host::Domain(name)) => !is_forbidden_domain(name),
            Some(url::Host::Ipv4(addr)) => self.allows_ip(addr.into()),
            Some(url::Host::Ipv6(addr)) => self.allows_ip(addr.into()),
            None => false,
        }
    }
}

fn is_forbidden_domain(name: &str) -> bool {
    let lower = name.trim_end_matches('.').to_ascii_lowercase();
    // A single label (`router`, `nas`) is completed with the local search domain.
    if !lower.contains('.') {
        return true;
    }
    LOCAL_SUFFIXES.iter().any(|suffix| {
        lower == *suffix
            || lower
                .strip_suffix(suffix)
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(addr) => is_public_ipv4(addr),
        IpAddr::V6(addr) => is_public_ipv6(addr),
    }
}

fn is_public_ipv4(addr: Ipv4Addr) -> bool {
    let [first, second, third, _] = addr.octets();
    !(addr.is_loopback()
        || addr.is_private()
        || addr.is_link_local()
        || addr.is_multicast()
        || addr.is_documentation()
        || first == 0 // 0.0.0.0/8 "this network"
        || (first == 100 && (second & 0xc0) == 64) // 100.64.0.0/10 CGNAT
        || (first == 192 && second == 0 && third == 0) // 192.0.0.0/24 IETF assignments
        || (first == 198 && (second & 0xfe) == 18) // 198.18.0.0/15 benchmarking
        || first >= 240) // 240.0.0.0/4 reserved, incl. broadcast
}

fn is_public_ipv6(addr: Ipv6Addr) -> bool {
    if addr.is_loopback() || addr.is_unspecified() || addr.is_multicast() {
        return false;
    }
    let seg = addr.segments();
    // fc00::/7 unique local, fe80::/10 link-local, fec0::/10 site-local (deprecated)
    if (seg[0] & 0xfe00) == 0xfc00 || (seg[0] & 0xffc0) == 0xfe80 || (seg[0] & 0xffc0) == 0xfec0 {
        return false;
    }
    // 2001:db8::/32 documentation, 2001::/32 Teredo (embeds an obfuscated IPv4)
    if seg[0] == 0x2001 && (seg[1] == 0x0db8 || seg[1] == 0) {
        return false;
    }
    // 64:ff9b:1::/48 local-use NAT64
    if seg[0] == 0x64 && seg[1] == 0xff9b && seg[2] == 1 {
        return false;
    }
    match embedded_ipv4(addr) {
        Some(v4) => is_public_ipv4(v4),
        None => true,
    }
}

/// IPv4 address carried inside an IPv6 one: IPv4-mapped `::ffff:a.b.c.d`,
/// IPv4-compatible `::a.b.c.d`, NAT64 `64:ff9b::a.b.c.d`, 6to4 `2002:aabb:ccdd::`.
fn embedded_ipv4(addr: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = addr.to_ipv4() {
        return Some(v4);
    }
    let seg = addr.segments();
    let oct = addr.octets();
    if seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return Some(Ipv4Addr::new(oct[12], oct[13], oct[14], oct[15]));
    }
    if seg[0] == 0x2002 {
        return Some(Ipv4Addr::new(oct[2], oct[3], oct[4], oct[5]));
    }
    None
}

/// Keep only the addresses `reach` allows; fail when none is left.
fn filter_addrs(
    host: &str,
    resolved: impl IntoIterator<Item = SocketAddr>,
    reach: Reach,
) -> Result<Vec<SocketAddr>, std::io::Error> {
    let allowed: Vec<SocketAddr> = resolved
        .into_iter()
        .filter(|addr| reach.allows_ip(addr.ip()))
        .collect();
    if allowed.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{host} resolves only to non-public addresses"),
        ));
    }
    Ok(allowed)
}

/// DNS resolver that drops non-public addresses, so a public name pointing
/// (or rebinding) to `192.168.1.1` cannot be reached.
struct GuardedResolver {
    reach: Reach,
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let reach = self.reach;
        Box::pin(async move {
            let resolved = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let allowed = filter_addrs(&host, resolved, reach)?;
            Ok(Box::new(allowed.into_iter()) as Addrs)
        })
    }
}

/// Split a `Content-Type` value into its lower-cased essence and charset.
fn parse_content_type(value: &str) -> (String, Option<String>) {
    let mut parts = value.split(';');
    let essence = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let charset = parts.find_map(|param| {
        let (key, val) = param.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| val.trim().trim_matches('"').to_string())
    });
    (essence, charset)
}

fn is_html(essence: &str) -> bool {
    matches!(essence, "text/html" | "application/xhtml+xml")
}

/// HTTP client for URLs that come from feed content. See the module docs.
#[derive(Clone)]
pub(crate) struct SafeClient {
    http: Client,
    reach: Reach,
}

impl SafeClient {
    pub(crate) fn new(timeout: Duration) -> Result<Self, FrustError> {
        Self::with_reach(timeout, Reach::PUBLIC)
    }

    /// Client that may also reach 127.0.0.1, for tests against a local server.
    #[cfg(test)]
    fn allowing_loopback(timeout: Duration) -> Result<Self, FrustError> {
        Self::with_reach(
            timeout,
            Reach {
                allow_loopback: true,
            },
        )
    }

    fn with_reach(timeout: Duration, reach: Reach) -> Result<Self, FrustError> {
        let policy = redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if reach.allows_url(attempt.url()) {
                attempt.follow()
            } else {
                let msg = format!("redirect to non-public target {}", attempt.url());
                attempt.error(msg)
            }
        });
        let http = Client::builder()
            .timeout(timeout)
            .user_agent(concat!("frust/", env!("CARGO_PKG_VERSION")))
            .redirect(policy)
            .dns_resolver(Arc::new(GuardedResolver { reach }))
            // A proxy would resolve and connect on our behalf, bypassing the resolver.
            .no_proxy()
            .build()?;
        Ok(Self { http, reach })
    }

    /// Fetch an HTML page, decoded to a string, refusing non-public targets.
    pub(crate) async fn fetch_page(&self, url: &str) -> Result<String, FrustError> {
        self.fetch_page_capped(url, MAX_PAGE_BYTES).await
    }

    async fn fetch_page_capped(&self, url: &str, max_bytes: u64) -> Result<String, FrustError> {
        let parsed = Url::parse(url).map_err(|e| FrustError::Scrape(format!("{url}: {e}")))?;
        if !self.reach.allows_url(&parsed) {
            return Err(FrustError::Scrape(format!(
                "refusing non-public or non-http target {url}"
            )));
        }

        let response = self
            .http
            .get(parsed)
            .header(
                header::ACCEPT,
                "text/html, application/xhtml+xml;q=0.9, */*;q=0.1",
            )
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(FrustError::Scrape(format!("{url}: HTTP {status}")));
        }

        let (essence, charset) = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(parse_content_type)
            .unwrap_or_default();
        // A missing Content-Type is tolerated; an explicit non-HTML one is not.
        if !essence.is_empty() && !is_html(&essence) {
            return Err(FrustError::Scrape(format!(
                "{url}: unexpected content type {essence}"
            )));
        }

        let Some(body) = read_bounded_body(response, max_bytes).await? else {
            return Err(FrustError::Scrape(format!(
                "{url}: body exceeds {max_bytes} bytes"
            )));
        };
        let encoding = charset
            .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
            .unwrap_or(encoding_rs::UTF_8);
        Ok(encoding.decode(&body).0.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn is_safe(url: &str) -> bool {
        Url::parse(url).is_ok_and(|u| Reach::PUBLIC.allows_url(&u))
    }

    // --- URL guard ---

    #[test]
    fn test_guard_accepts_public_https_domain() {
        assert!(is_safe("https://example.com/article"));
        assert!(is_safe("http://blog.rust-lang.org/2024/x"));
    }

    #[test]
    fn test_guard_rejects_non_http_schemes() {
        assert!(!is_safe("file:///etc/passwd"));
        assert!(!is_safe("ftp://ftp.example.com/x"));
        assert!(!is_safe("gopher://example.com/"));
        assert!(!is_safe("javascript:alert(1)"));
    }

    #[test]
    fn test_guard_rejects_local_domains() {
        for url in [
            "http://localhost/x",
            "http://LOCALHOST/x",
            "http://localhost./x",
            "http://foo.localhost/x",
            "http://router.local/x",
            "http://admin.internal/x",
            "http://openwrt.lan/x",
            "http://nas.home.arpa/x",
            "http://box.home/x",
            "http://wiki.corp/x",
            "http://router/x",
        ] {
            assert!(!is_safe(url), "should reject local domain: {url}");
        }
    }

    #[test]
    fn test_guard_does_not_overmatch_suffixes() {
        // "lan" / "home" as part of a label, not a suffix label.
        assert!(is_safe("https://milan.it/x"));
        assert!(is_safe("https://myhome.com/x"));
    }

    #[test]
    fn test_guard_rejects_non_public_ipv4_literals() {
        for host in [
            "127.0.0.1",
            "127.1.2.3",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254", // cloud metadata
            "0.0.0.0",
            "0.1.2.3",
            "100.64.0.1", // CGNAT
            "100.127.255.255",
            "192.0.0.1",
            "192.0.2.1", // documentation
            "198.18.0.1",
            "198.19.255.255",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            let url = format!("http://{host}/x");
            assert!(!is_safe(&url), "should reject IPv4: {url}");
        }
    }

    #[test]
    fn test_guard_rejects_obfuscated_ipv4_literals() {
        // Parsed by the URL crate into 127.0.0.1.
        assert!(!is_safe("http://2130706433/x"));
        assert!(!is_safe("http://0x7f.1/x"));
    }

    #[test]
    fn test_guard_accepts_public_ipv4_literals() {
        assert!(is_safe("http://8.8.8.8/x"));
        assert!(is_safe("http://1.1.1.1/x"));
        assert!(is_safe("http://100.128.0.1/x")); // just above CGNAT
        assert!(is_safe("http://198.20.0.1/x")); // just above benchmarking
    }

    #[test]
    fn test_guard_rejects_non_public_ipv6() {
        for host in [
            "[::1]",
            "[::]",
            "[fe80::1]",
            "[fec0::1]",
            "[fc00::1]",
            "[fd12:3456::1]",
            "[2001:db8::1]",
            "[2001:0:4136:e378::1]", // Teredo
            "[64:ff9b:1::1]",
            "[::ffff:127.0.0.1]",
            "[::ffff:192.168.1.1]",
            "[::192.168.1.1]",     // IPv4-compatible
            "[64:ff9b::10.0.0.1]", // NAT64 to private
            "[64:ff9b::c0a8:101]", // NAT64 to 192.168.1.1
            "[2002:c0a8:101::1]",  // 6to4 of 192.168.1.1
            "[2002:7f00:1::1]",    // 6to4 of 127.0.0.1
        ] {
            let url = format!("http://{host}/x");
            assert!(!is_safe(&url), "should reject IPv6: {url}");
        }
    }

    #[test]
    fn test_guard_accepts_public_ipv6() {
        assert!(is_safe("http://[2606:4700:4700::1111]/x"));
        assert!(is_safe("http://[64:ff9b::808:808]/x")); // NAT64 to 8.8.8.8
        assert!(is_safe("http://[2002:808:808::1]/x")); // 6to4 of 8.8.8.8
    }

    #[test]
    fn test_guard_rejects_malformed_urls() {
        assert!(!is_safe(""));
        assert!(!is_safe("not a url"));
        assert!(!is_safe("http://"));
    }

    // --- resolver ---

    #[test]
    fn test_filter_addrs_keeps_only_public() {
        let resolved: Vec<SocketAddr> = vec![
            "192.168.1.1:0".parse().unwrap(),
            "93.184.216.34:0".parse().unwrap(),
            "[::1]:0".parse().unwrap(),
        ];
        let kept = filter_addrs("example.com", resolved, Reach::PUBLIC).unwrap();
        let public: SocketAddr = "93.184.216.34:0".parse().unwrap();
        assert_eq!(kept, vec![public]);
    }

    #[test]
    fn test_filter_addrs_fails_when_all_private() {
        let resolved: Vec<SocketAddr> = vec!["10.0.0.1:0".parse().unwrap()];
        assert!(filter_addrs("evil.example", resolved, Reach::PUBLIC).is_err());
    }

    #[tokio::test]
    async fn test_resolver_refuses_name_resolving_to_loopback() {
        // `localhost` is resolved offline from /etc/hosts. Going through the
        // resolver directly bypasses the domain-name check, as a public name
        // pointing at 127.0.0.1 would.
        let resolver = GuardedResolver {
            reach: Reach::PUBLIC,
        };
        let name: Name = "localhost".parse().unwrap();
        assert!(resolver.resolve(name).await.is_err());
    }

    // --- content type ---

    #[test]
    fn test_parse_content_type() {
        assert_eq!(
            parse_content_type("Text/HTML; Charset=\"ISO-8859-1\""),
            ("text/html".to_string(), Some("ISO-8859-1".to_string()))
        );
        assert_eq!(
            parse_content_type("application/xhtml+xml"),
            ("application/xhtml+xml".to_string(), None)
        );
    }

    // --- fetch_page against a local server ---

    /// Raw HTTP response with `Content-Length` and `Connection: close`.
    fn http_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (name, value) in headers {
            out.push_str(&format!("{name}: {value}\r\n"));
        }
        out.push_str("\r\n");
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    fn html(body: &str) -> Vec<u8> {
        http_response(
            "200 OK",
            &[("Content-Type", "text/html; charset=utf-8")],
            body.as_bytes(),
        )
    }

    fn redirect_to(location: &str) -> Vec<u8> {
        http_response("302 Found", &[("Location", location)], b"")
    }

    /// Serve `respond(path, port)` on 127.0.0.1 and return the port.
    async fn spawn_server(respond: fn(&str, u16) -> Vec<u8>) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let mut len = 0;
                    while len < buf.len() {
                        let read = stream.read(&mut buf[len..]).await.unwrap_or(0);
                        if read == 0 {
                            break;
                        }
                        len += read;
                        if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let head = String::from_utf8_lossy(&buf[..len]);
                    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let _ = stream.write_all(&respond(&path, port)).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        port
    }

    fn test_client() -> SafeClient {
        SafeClient::allowing_loopback(Duration::from_secs(5)).unwrap()
    }

    #[tokio::test]
    async fn test_fetch_page_returns_html_body() {
        let port = spawn_server(|_, _| html("<main>hello</main>")).await;
        let page = test_client()
            .fetch_page(&format!("http://127.0.0.1:{port}/"))
            .await
            .unwrap();
        assert_eq!(page, "<main>hello</main>");
    }

    #[tokio::test]
    async fn test_fetch_page_production_client_refuses_loopback() {
        let port = spawn_server(|_, _| html("secret")).await;
        let client = SafeClient::new(Duration::from_secs(5)).unwrap();
        for url in [
            format!("http://127.0.0.1:{port}/"),
            format!("http://localhost:{port}/"),
        ] {
            let result = client.fetch_page(&url).await;
            assert!(
                matches!(result, Err(FrustError::Scrape(_))),
                "{url} must be refused before any request"
            );
        }
    }

    #[tokio::test]
    async fn test_fetch_page_follows_redirect_to_allowed_target() {
        let port = spawn_server(|path, port| match path {
            "/start" => redirect_to(&format!("http://127.0.0.1:{port}/final")),
            _ => html("final"),
        })
        .await;
        let page = test_client()
            .fetch_page(&format!("http://127.0.0.1:{port}/start"))
            .await
            .unwrap();
        assert_eq!(page, "final");
    }

    #[tokio::test]
    async fn test_fetch_page_refuses_redirect_to_private_target() {
        let port = spawn_server(|_, _| redirect_to("http://192.168.1.1/cgi-bin/admin")).await;
        let result = test_client()
            .fetch_page(&format!("http://127.0.0.1:{port}/"))
            .await;
        assert!(
            matches!(&result, Err(FrustError::Http(e)) if e.is_redirect()),
            "got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_fetch_page_refuses_redirect_to_local_domain() {
        let port = spawn_server(|_, _| redirect_to("http://openwrt.lan/")).await;
        let result = test_client()
            .fetch_page(&format!("http://127.0.0.1:{port}/"))
            .await;
        assert!(matches!(&result, Err(FrustError::Http(e)) if e.is_redirect()));
    }

    #[tokio::test]
    async fn test_fetch_page_stops_redirect_loops() {
        let port = spawn_server(|_, port| redirect_to(&format!("http://127.0.0.1:{port}/"))).await;
        let result = test_client()
            .fetch_page(&format!("http://127.0.0.1:{port}/"))
            .await;
        assert!(matches!(&result, Err(FrustError::Http(e)) if e.is_redirect()));
    }

    #[tokio::test]
    async fn test_fetch_page_rejects_error_status() {
        let port = spawn_server(|_, _| {
            http_response(
                "404 Not Found",
                &[("Content-Type", "text/html")],
                b"<h1>404</h1>",
            )
        })
        .await;
        let result = test_client()
            .fetch_page(&format!("http://127.0.0.1:{port}/"))
            .await;
        assert!(matches!(result, Err(FrustError::Scrape(_))));
    }

    #[tokio::test]
    async fn test_fetch_page_rejects_non_html_content() {
        let port = spawn_server(|_, _| {
            http_response(
                "200 OK",
                &[("Content-Type", "application/octet-stream")],
                b"\x00\x01",
            )
        })
        .await;
        let result = test_client()
            .fetch_page(&format!("http://127.0.0.1:{port}/"))
            .await;
        assert!(matches!(result, Err(FrustError::Scrape(_))));
    }

    #[tokio::test]
    async fn test_fetch_page_rejects_body_over_cap() {
        let port = spawn_server(|_, _| html(&"x".repeat(2048))).await;
        let result = test_client()
            .fetch_page_capped(&format!("http://127.0.0.1:{port}/"), 1024)
            .await;
        assert!(matches!(result, Err(FrustError::Scrape(_))));
    }

    #[tokio::test]
    async fn test_fetch_page_decodes_declared_charset() {
        let port = spawn_server(|_, _| {
            http_response(
                "200 OK",
                &[("Content-Type", "text/html; charset=ISO-8859-1")],
                b"caf\xe9",
            )
        })
        .await;
        let page = test_client()
            .fetch_page(&format!("http://127.0.0.1:{port}/"))
            .await
            .unwrap();
        assert_eq!(page, "café");
    }
}
