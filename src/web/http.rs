use std::fmt::Write as _;
use std::io::{self, Read, Write};

const MAX_HEAD: usize = 16 * 1024;
const MAX_HEADERS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
}

impl Request {
    pub fn parse(head: &[u8]) -> Option<Self> {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut request = httparse::Request::new(&mut headers);
        if !request.parse(head).ok()?.is_complete() {
            return None;
        }
        let (path, query) = request.path?.split_once('?').unwrap_or((request.path?, ""));
        let query = query
            .split('&')
            .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
            .filter(|(key, _)| !key.is_empty())
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let headers = request
            .headers
            .iter()
            .map(|h| (h.name.to_ascii_lowercase(), String::from_utf8_lossy(h.value).trim().to_string()))
            .collect();
        Some(Self { method: request.method?.to_string(), path: path.to_string(), query, headers })
    }

    pub fn read(stream: &mut impl Read) -> io::Result<Option<Self>> {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() >= MAX_HEAD || stream.read(&mut byte)? == 0 {
                return Ok(None);
            }
            head.push(byte[0]);
        }
        Ok(Self::parse(&head))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    pub fn query(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .filter(|(key, _)| key == "cookie")
            .flat_map(|(_, value)| value.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    pub fn same_origin(&self) -> bool {
        let Some(origin) = self.header("origin") else { return false };
        let Some((scheme, host)) = origin.split_once("://") else { return false };
        if !matches!(scheme, "http" | "https") {
            return false;
        }
        [self.header("x-forwarded-host"), self.header("host")].into_iter().flatten().any(|h| h == host)
    }

    pub fn forwarded(&self) -> bool {
        ["x-forwarded-for", "x-forwarded-host", "tailscale-headers-info"].iter().any(|h| self.header(h).is_some())
    }

    pub fn wants_websocket(&self) -> bool {
        self.header("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"))
    }
}

pub struct Response {
    status: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Response {
    pub fn new(status: &'static str, kind: &'static str, body: impl Into<Vec<u8>>) -> Self {
        let headers = vec![
            ("Content-Type", kind.into()),
            ("Cache-Control", "no-store".into()),
            ("X-Content-Type-Options", "nosniff".into()),
            ("X-Frame-Options", "DENY".into()),
            ("Referrer-Policy", "no-referrer".into()),
        ];
        Self { status, headers, body: body.into() }
    }

    pub fn html(status: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self::new(status, "text/html; charset=utf-8", body).with(
            "Content-Security-Policy",
            "default-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' wss: ws:; frame-ancestors 'none'"
                .into(),
        )
    }

    pub fn redirect(to: &str) -> Self {
        Self::new("303 See Other", "text/plain; charset=utf-8", "").with("Location", to.into())
    }

    pub fn with(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }

    pub fn send(&self, out: &mut impl Write) -> io::Result<()> {
        let mut head =
            format!("HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n", self.status, self.body.len());
        for (name, value) in &self.headers {
            let _ = write!(head, "{name}: {value}\r\n");
        }
        head.push_str("\r\n");
        out.write_all(head.as_bytes())?;
        out.write_all(&self.body)?;
        out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(lines: &[&str]) -> Request {
        Request::parse(format!("{}\r\n\r\n", lines.join("\r\n")).as_bytes()).expect("a request")
    }

    #[test]
    fn reads_the_path_query_and_headers() {
        let r = request(&["GET /pair?t=abc&x HTTP/1.1", "Host: mac:8443", "Tailscale-User-Login: ada@example.com"]);

        assert_eq!(
            (r.method.as_str(), r.path.as_str(), r.query("t"), r.query("x")),
            ("GET", "/pair", Some("abc"), Some(""))
        );
        assert_eq!(r.header("tailscale-user-login"), Some("ada@example.com"));
    }

    #[test]
    fn finds_a_cookie_among_others() {
        let r = request(&["GET / HTTP/1.1", "Cookie: theme=dark; cornercase=id.secret", "Cookie: other=1"]);

        assert_eq!((r.cookie("cornercase"), r.cookie("other"), r.cookie("none")), (Some("id.secret"), Some("1"), None));
    }

    #[test]
    fn an_origin_must_match_the_host_it_was_sent_to() {
        let tailnet = request(&["GET /ws HTTP/1.1", "Host: mac.ts.net:8443", "Origin: https://mac.ts.net:8443"]);
        let forwarded = request(&[
            "GET /ws HTTP/1.1",
            "Host: 127.0.0.1:47681",
            "X-Forwarded-Host: mac.ts.net:8443",
            "Origin: https://mac.ts.net:8443",
        ]);
        let other = request(&["GET /ws HTTP/1.1", "Host: mac.ts.net:8443", "Origin: https://evil.example"]);
        let scheme = request(&["GET /ws HTTP/1.1", "Host: mac.ts.net:8443", "Origin: file://mac.ts.net:8443"]);
        let none = request(&["GET /ws HTTP/1.1", "Host: mac.ts.net:8443"]);

        assert_eq!(
            [
                tailnet.same_origin(),
                forwarded.same_origin(),
                other.same_origin(),
                scheme.same_origin(),
                none.same_origin()
            ],
            [true, true, false, false, false]
        );
    }

    #[test]
    fn a_proxied_request_says_so() {
        let local = request(&["GET / HTTP/1.1", "Host: 127.0.0.1:47681"]);
        let proxied = request(&["GET / HTTP/1.1", "Host: mac.ts.net:8443", "X-Forwarded-For: 100.64.0.2"]);

        assert_eq!((local.forwarded(), proxied.forwarded()), (false, true));
    }

    #[test]
    fn a_head_that_never_ends_is_dropped() {
        let endless = vec![b'a'; MAX_HEAD + 10];

        assert_eq!(Request::read(&mut endless.as_slice()).expect("read"), None);
    }

    #[test]
    fn a_response_says_its_length_and_status() {
        let mut out = Vec::new();
        Response::redirect("/").with("Set-Cookie", "a=b".into()).send(&mut out).expect("sent");

        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.starts_with("HTTP/1.1 303 See Other\r\nContent-Length: 0\r\n"));
        assert!(text.contains("\r\nLocation: /\r\n") && text.contains("\r\nSet-Cookie: a=b\r\n"));
    }
}
