//! Home Assistant REST API: entity state and the switch services.
//! `HA_BASE` (server URL without `/api`) and `HA_KEY` (long-lived token) come from the
//! environment only. Errors never carry the token, the URL or a response body.

use std::time::Duration;

use serde_json::{Value, json};

use crate::http;

/// A configured client.
pub(crate) struct Client {
    host: String,
    port: u16,
    prefix: String,
    key: String,
    timeout: Duration,
}

impl Client {
    pub(crate) fn from_env(timeout: Duration) -> Result<Self, String> {
        let base = std::env::var("HA_BASE").unwrap_or_default();
        let key = std::env::var("HA_KEY").unwrap_or_default();
        if base.is_empty() || key.is_empty() {
            return Err("set HA_BASE and HA_KEY in your environment (loaded by direnv)".to_owned());
        }
        let (host, port, prefix) = parse_base(&base)?;
        if key.trim().is_empty() || key.contains(['\r', '\n', '\0']) {
            return Err("HA_KEY must contain a valid access token".to_owned());
        }
        Ok(Self {
            host,
            port,
            prefix,
            key,
            timeout,
        })
    }

    /// `GET /api/states/<entity>`: the entity object, with a string `state`.
    pub(crate) fn state(&self, entity: &str) -> Result<Value, String> {
        let v = self.request("GET", &format!("states/{entity}"), None)?;
        if v.get("state").and_then(Value::as_str).is_none() {
            return Err("Home Assistant returned an invalid entity state".to_owned());
        }
        Ok(v)
    }

    /// `POST /api/services/switch/turn_on|turn_off`.
    pub(crate) fn switch(&self, entity: &str, on: bool) -> Result<(), String> {
        let service = if on { "turn_on" } else { "turn_off" };
        self.request(
            "POST",
            &format!("services/switch/{service}"),
            Some(&json!({ "entity_id": entity })),
        )
        .map(drop)
    }

    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let body = body.map(Value::to_string).unwrap_or_default();
        let host_header = if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        };
        let request = format!(
            "{method} {}/api/{path} HTTP/1.1\r\nHost: {host_header}\r\nAuthorization: Bearer {}\r\n\
             Content-Type: application/json\r\nAccept: application/json\r\nConnection: close\r\n\
             Content-Length: {}\r\n\r\n{body}",
            self.prefix,
            self.key,
            body.len()
        );
        let connect = self.host.trim_start_matches('[').trim_end_matches(']');
        let raw = http::exchange(connect, self.port, request.as_bytes(), self.timeout)
            .map_err(|e| format!("Home Assistant connection failed ({})", e.kind()))?;
        let response =
            http::parse_response(&raw).map_err(|_| "Home Assistant returned an invalid HTTP response".to_owned())?;
        if !(200..300).contains(&response.status) {
            let hint = match response.status {
                401 => "check HA_KEY",
                403 => "access denied",
                404 => "entity or endpoint not found",
                _ => "request failed; check HA_BASE and server",
            };
            return Err(format!("Home Assistant HTTP {}: {hint}", response.status));
        }
        serde_json::from_slice(&response.body).map_err(|_| "Home Assistant returned invalid JSON".to_owned())
    }
}

/// `http://host[:port][/prefix]`; https, credentials, query and fragment are refused.
fn parse_base(base: &str) -> Result<(String, u16, String), String> {
    let bad = || "HA_BASE must be an http:// URL without credentials, query, or fragment".to_owned();
    if base.starts_with("https://") {
        return Err(
            "HA_BASE uses https, which lab-power does not speak (no TLS library): use the LAN http:// URL".to_owned(),
        );
    }
    let rest = base.strip_prefix("http://").ok_or_else(bad)?;
    if rest.contains(['@', '?', '#']) || rest.contains(char::is_whitespace) {
        return Err(bad());
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (rest.get(..i).unwrap_or_default(), rest.get(i..).unwrap_or_default()),
        None => (rest, ""),
    };
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (addr, after) = v6.split_once(']').ok_or_else(bad)?;
        (format!("[{addr}]"), after.strip_prefix(':'))
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p)),
            None => (authority.to_owned(), None),
        }
    };
    let plain = host.trim_start_matches('[').trim_end_matches(']');
    if plain.is_empty() || !plain.chars().all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c)) {
        return Err(bad());
    }
    let port = match port {
        None => 80,
        Some(p) => p.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(bad)?,
    };
    Ok((host, port, path.trim_end_matches('/').to_owned()))
}

#[cfg(test)]
mod tests {
    use super::parse_base;

    #[test]
    fn base_urls() {
        assert_eq!(
            parse_base("http://ha.lan:8123/").unwrap(),
            ("ha.lan".into(), 8123, String::new())
        );
        assert_eq!(
            parse_base("http://10.0.0.2").unwrap(),
            ("10.0.0.2".into(), 80, String::new())
        );
        assert_eq!(
            parse_base("http://[fd00::1]:8123/ha").unwrap(),
            ("[fd00::1]".into(), 8123, "/ha".into())
        );
        for bad in [
            "ftp://x",
            "http://u:p@h",
            "http://h/?q",
            "http://h#f",
            "http://",
            "http://h:0",
            "http://h:x",
            "https://h",
        ] {
            assert!(parse_base(bad).is_err(), "{bad}");
        }
        assert!(parse_base("https://h").unwrap_err().contains("https"));
    }
}
