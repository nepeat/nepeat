//! Per-port scrollback: the last `cap` bytes, in RAM. Grows as the port talks, so silent
//! ports cost nothing.

use std::collections::VecDeque;

pub struct Ring {
    buf: VecDeque<u8>,
    cap: usize,
}

impl Ring {
    pub fn new(cap: usize) -> Ring {
        Ring {
            buf: VecDeque::new(),
            cap,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        if data.len() >= self.cap {
            self.buf.clear();
            self.buf.extend(&data[data.len() - self.cap..]);
            return;
        }
        let over = (self.buf.len() + data.len()).saturating_sub(self.cap);
        self.buf.drain(..over);
        self.buf.extend(data);
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// The contents, oldest first, in at most `chunk`-sized pieces.
    pub fn chunks(&self, chunk: usize) -> Vec<Vec<u8>> {
        let (a, b) = self.buf.as_slices();
        let mut all = Vec::with_capacity(self.buf.len());
        all.extend_from_slice(a);
        all.extend_from_slice(b);
        all.chunks(chunk.max(1)).map(|c| c.to_vec()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(r: &Ring) -> Vec<u8> {
        r.chunks(3).concat()
    }

    #[test]
    fn keeps_the_last_cap_bytes() {
        let mut r = Ring::new(5);
        r.push(b"abc");
        assert_eq!(flat(&r), b"abc");
        r.push(b"defg");
        assert_eq!(flat(&r), b"cdefg");
        r.push(b"0123456789");
        assert_eq!(flat(&r), b"56789");
        assert_eq!(r.len(), 5);
    }
}
