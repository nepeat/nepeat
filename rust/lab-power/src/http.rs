//! A minimal HTTP/1.1 client over `std::net`: one request per connection, `Connection: close`,
//! no redirects followed, fixed and chunked bodies. Enough for Home Assistant's REST API on the LAN.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

const MAX_RESPONSE: usize = 4 << 20;

/// A parsed response.
#[derive(Debug)]
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
}

/// Sends `request` (a complete HTTP message) and reads the response, all within `timeout`.
pub(crate) fn exchange(host: &str, port: u16, request: &[u8], timeout: Duration) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + timeout;
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no address");
    let mut stream = None;
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, remaining(deadline)?) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = e,
        }
    }
    let mut stream = stream.ok_or(last)?;
    stream.set_write_timeout(Some(remaining(deadline)?))?;
    stream.write_all(request)?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(buf.get(..n).unwrap_or_default());
        if raw.len() > MAX_RESPONSE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "response too large"));
        }
        if complete(&raw) {
            break;
        }
    }
    Ok(raw)
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(io::Error::new(io::ErrorKind::TimedOut, "timed out"))
    } else {
        Ok(left)
    }
}

/// True once a response with a Content-Length has all of its body (else read to EOF).
fn complete(raw: &[u8]) -> bool {
    let Some(end) = find(raw, b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(raw.get(..end).unwrap_or_default()).to_ascii_lowercase();
    head.lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .is_some_and(|len| raw.len() >= end + 4 + len)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Splits status, headers and body; undoes chunked transfer encoding.
pub(crate) fn parse_response(raw: &[u8]) -> Result<Response, &'static str> {
    let end = find(raw, b"\r\n\r\n").ok_or("incomplete response")?;
    let head = std::str::from_utf8(raw.get(..end).ok_or("bad response")?).map_err(|_| "bad response head")?;
    let body = raw.get(end + 4..).unwrap_or_default();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("empty response")?;
    let mut parts = status_line.split(' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err("not an HTTP/1.x response");
    }
    let status = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("bad status line")?;
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        if name == "transfer-encoding" && value.contains("chunked") {
            chunked = true;
        } else if name == "content-length" {
            length = Some(value.parse::<usize>().map_err(|_| "bad content-length")?);
        }
    }
    let body = if chunked {
        dechunk(body)?
    } else if let Some(len) = length {
        body.get(..len).ok_or("truncated body")?.to_vec()
    } else {
        body.to_vec()
    };
    Ok(Response { status, body })
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();
    loop {
        let eol = find(data, b"\r\n").ok_or("truncated chunk")?;
        let size_text = std::str::from_utf8(data.get(..eol).unwrap_or_default()).map_err(|_| "bad chunk")?;
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| "bad chunk size")?;
        data = data.get(eol + 2..).unwrap_or_default();
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(data.get(..size).ok_or("truncated chunk")?);
        data = data.get(size..).unwrap_or_default();
        data = data.strip_prefix(b"\r\n").ok_or("bad chunk end")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_and_chunked_bodies() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}junk").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"{}"[..]));
        let r = parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n4;x=y\r\n\":1}\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(r.body, br#"{"a":1}"#);
        let r = parse_response(b"HTTP/1.0 302 Found\r\nLocation: /x\r\n\r\n").unwrap();
        assert_eq!(r.status, 302);
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n{}").is_err());
        assert!(parse_response(b"SSH-2.0-x\r\n\r\n").is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err());
        assert!(complete(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"));
        assert!(!complete(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{}"));
    }
}
