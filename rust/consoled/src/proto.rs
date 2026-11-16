//! Client <-> daemon framing over the unix socket: [type u8][len u32 BE][payload].
//!
//! client -> daemon: HELLO (JSON `Hello`), DATA (keystrokes), CMD (JSON `Cmd`)
//! daemon -> client: DATA (port output: scrollback, then live), INFO (a line for the user),
//!                   ERROR (then the daemon closes), LIST (JSON array of `PortStatus`)

use serde::{Deserialize, Serialize};

pub const HELLO: u8 = 1;
pub const DATA: u8 = 2;
pub const CMD: u8 = 3;
pub const INFO: u8 = 4;
pub const ERROR: u8 = 5;
pub const LIST: u8 = 6;

/// Larger frames are a protocol error (scrollback replay is chunked below this).
pub const MAX_FRAME: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    /// Port name or label; None asks for the port list.
    pub port: Option<String>,
    /// Ask for write access (granted if nobody holds it).
    #[serde(default)]
    pub write: bool,
    /// Who this is, for the others ("user@203.0.113.7").
    #[serde(default)]
    pub who: String,
    /// Replay the scrollback first.
    #[serde(default = "yes")]
    pub replay: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Cmd {
    /// Take write access (from whoever holds it).
    Take,
    /// Give write access up.
    Release,
    /// Send a serial break (writer only).
    Break,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortStatus {
    pub name: String,
    pub label: String,
    pub device: String,
    pub settings: String,
    pub up: bool,
    pub clients: usize,
    pub writer: Option<String>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

pub fn encode(kind: u8, payload: &[u8], out: &mut Vec<u8>) {
    out.push(kind);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
}

pub fn encode_json<T: Serialize>(kind: u8, value: &T, out: &mut Vec<u8>) {
    let body = serde_json::to_vec(value).expect("serialisable");
    encode(kind, &body, out);
}

/// Incremental decoder: push received bytes, pull complete frames.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Ok(None): need more bytes. Err: the peer is speaking nonsense.
    pub fn next(&mut self) -> Result<Option<(u8, Vec<u8>)>, String> {
        if self.buf.len() < 5 {
            return Ok(None);
        }
        let len = u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]]) as usize;
        if len > MAX_FRAME {
            return Err(format!("frame of {len} bytes"));
        }
        if self.buf.len() < 5 + len {
            return Ok(None);
        }
        let kind = self.buf[0];
        let payload = self.buf[5..5 + len].to_vec();
        self.buf.drain(..5 + len);
        Ok(Some((kind, payload)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_split_reads() {
        let mut out = Vec::new();
        encode(DATA, b"hello", &mut out);
        encode_json(CMD, &Cmd::Take, &mut out);
        let mut d = Decoder::default();
        for b in &out {
            d.push(std::slice::from_ref(b)); // one byte at a time
        }
        assert_eq!(d.next().unwrap(), Some((DATA, b"hello".to_vec())));
        let (k, p) = d.next().unwrap().unwrap();
        assert_eq!(k, CMD);
        assert_eq!(serde_json::from_slice::<Cmd>(&p).unwrap(), Cmd::Take);
        assert_eq!(d.next().unwrap(), None);
    }

    #[test]
    fn oversized_frame_is_an_error() {
        let mut d = Decoder::default();
        d.push(&[DATA, 0xff, 0xff, 0xff, 0xff]);
        assert!(d.next().is_err());
    }
}
