//! The SPICE agent protocol (spice-protocol's vd_agent.h), as far as the
//! clipboard needs it.
//!
//! On the virtio-serial port `com.redhat.spice.0` the stream is a series
//! of chunks (`VDIChunkHeader`: port, size; at most 2048 bytes of data)
//! carrying messages (`VDAgentMessage`: protocol 1, type, opaque, size,
//! then the data), a message possibly spread over several chunks. Both
//! sides announce their capabilities; with CLIPBOARD_BY_DEMAND (which
//! QEMU's qemu-vdagent needs before it shares the clipboard) the side
//! whose clipboard changed sends CLIPBOARD_GRAB with the types it has,
//! the other asks for one with CLIPBOARD_REQUEST and gets CLIPBOARD;
//! CLIPBOARD_RELEASE says the clipboard is empty again. The agent does not
//! announce CLIPBOARD_SELECTION or CLIPBOARD_GRAB_SERIAL, so messages have
//! no selection or serial prefix, nor a line-end capability: it converts
//! line ends itself (hosts get LF).

use alloc::vec::Vec;

/// VDP_CLIENT_PORT: messages between the agent and the client (here QEMU).
pub const PORT_CLIENT: u32 = 1;
pub const PROTOCOL: u32 = 1;
/// VD_AGENT_MAX_DATA_SIZE: data in one chunk.
pub const MAX_CHUNK: usize = 2048;
/// Larger messages are refused (the stream starts afresh).
pub const MAX_MESSAGE: usize = 32 * 1024 * 1024;

pub const MOUSE_STATE: u32 = 1;
pub const MONITORS_CONFIG: u32 = 2;
pub const REPLY: u32 = 3;
pub const CLIPBOARD: u32 = 4;
pub const DISPLAY_CONFIG: u32 = 5;
pub const ANNOUNCE_CAPABILITIES: u32 = 6;
pub const CLIPBOARD_GRAB: u32 = 7;
pub const CLIPBOARD_REQUEST: u32 = 8;
pub const CLIPBOARD_RELEASE: u32 = 9;

pub const CAP_CLIPBOARD: u32 = 3;
pub const CAP_CLIPBOARD_BY_DEMAND: u32 = 5;

/// Clipboard types.
pub const TYPE_NONE: u32 = 0;
pub const TYPE_UTF8_TEXT: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
pub struct Message {
    pub ty: u32,
    pub data: Vec<u8>,
}

impl Message {
    /// The data's 32-bit words.
    pub fn words(&self) -> impl Iterator<Item = u32> + '_ {
        self.data.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
    }
}

/// A message as chunks, ready to write.
pub fn encode(ty: u32, data: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(20 + data.len());
    msg.extend_from_slice(&PROTOCOL.to_le_bytes());
    msg.extend_from_slice(&ty.to_le_bytes());
    msg.extend_from_slice(&0u64.to_le_bytes());
    msg.extend_from_slice(&(data.len() as u32).to_le_bytes());
    msg.extend_from_slice(data);
    let mut out = Vec::with_capacity(msg.len() + 8 * (msg.len() / MAX_CHUNK + 1));
    for c in msg.chunks(MAX_CHUNK) {
        out.extend_from_slice(&PORT_CLIENT.to_le_bytes());
        out.extend_from_slice(&(c.len() as u32).to_le_bytes());
        out.extend_from_slice(c);
    }
    out
}

fn words(ws: &[u32]) -> Vec<u8> {
    ws.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// ANNOUNCE_CAPABILITIES (`request`: the other side should announce its).
pub fn announce(request: bool) -> Vec<u8> {
    let caps = (1 << CAP_CLIPBOARD) | (1 << CAP_CLIPBOARD_BY_DEMAND);
    encode(ANNOUNCE_CAPABILITIES, &words(&[request as u32, caps]))
}

/// CLIPBOARD_GRAB of text.
pub fn grab_text() -> Vec<u8> {
    encode(CLIPBOARD_GRAB, &words(&[TYPE_UTF8_TEXT]))
}

pub fn release() -> Vec<u8> {
    encode(CLIPBOARD_RELEASE, &[])
}

pub fn request(ty: u32) -> Vec<u8> {
    encode(CLIPBOARD_REQUEST, &words(&[ty]))
}

/// CLIPBOARD with UTF-8 text, or TYPE_NONE without data (nothing to give).
pub fn clipboard(text: Option<&[u8]>) -> Vec<u8> {
    let mut d = words(&[if text.is_some() { TYPE_UTF8_TEXT } else { TYPE_NONE }]);
    d.extend_from_slice(text.unwrap_or(&[]));
    encode(CLIPBOARD, &d)
}

/// Reassembles messages from the chunks read.
#[derive(Default)]
pub struct Parser {
    /// Bytes of a chunk header read so far.
    head: Vec<u8>,
    /// Data left in the current chunk.
    left: usize,
    /// The message being assembled.
    msg: Vec<u8>,
}

impl Parser {
    pub fn new() -> Parser {
        Parser::default()
    }

    /// Take bytes read from the port; returns the messages completed.
    pub fn feed(&mut self, mut b: &[u8]) -> Vec<Message> {
        let mut out = Vec::new();
        while !b.is_empty() {
            if self.left == 0 {
                let n = (8 - self.head.len()).min(b.len());
                self.head.extend_from_slice(&b[..n]);
                b = &b[n..];
                if self.head.len() == 8 {
                    self.left = u32::from_le_bytes([self.head[4], self.head[5], self.head[6], self.head[7]]) as usize;
                    self.head.clear();
                }
                continue;
            }
            let n = self.left.min(b.len());
            self.msg.extend_from_slice(&b[..n]);
            self.left -= n;
            b = &b[n..];
            while self.msg.len() >= 20 {
                let size = u32::from_le_bytes([self.msg[16], self.msg[17], self.msg[18], self.msg[19]]) as usize;
                if size > MAX_MESSAGE {
                    self.msg.clear();
                    break;
                }
                if self.msg.len() < 20 + size {
                    break;
                }
                let ty = u32::from_le_bytes([self.msg[4], self.msg[5], self.msg[6], self.msg[7]]);
                let data = self.msg[20..20 + size].to_vec();
                self.msg.drain(..20 + size);
                out.push(Message { ty, data });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_in_pieces() {
        let big: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let mut stream = clipboard(Some(&big));
        stream.extend(grab_text());
        stream.extend(release());
        // Three chunks for the big message.
        assert_eq!(u32::from_le_bytes(stream[4..8].try_into().unwrap()) as usize, MAX_CHUNK);
        let mut p = Parser::new();
        let mut got = Vec::new();
        for piece in stream.chunks(7) {
            got.extend(p.feed(piece));
        }
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].ty, CLIPBOARD);
        assert_eq!(got[0].words().next(), Some(TYPE_UTF8_TEXT));
        assert_eq!(&got[0].data[4..], &big[..]);
        assert_eq!(got[1], Message { ty: CLIPBOARD_GRAB, data: words(&[TYPE_UTF8_TEXT]) });
        assert_eq!(got[2], Message { ty: CLIPBOARD_RELEASE, data: Vec::new() });
    }

    #[test]
    fn announce_bits() {
        let mut p = Parser::new();
        let m = p.feed(&announce(true)).pop().unwrap();
        assert_eq!(m.ty, ANNOUNCE_CAPABILITIES);
        assert_eq!(m.words().collect::<Vec<_>>(), [1, 0x28]);
    }
}
