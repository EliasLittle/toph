use serde::{Deserialize, Serialize};

use iroh::endpoint::{RecvStream, SendStream};

/// ALPN for this protocol. Bump the version suffix on any breaking wire change.
pub const ALPN: &[u8] = b"toph/0";

/// Hard cap on any single framed message to prevent OOM from a malicious peer.
pub const MAX_MESSAGE_SIZE: u32 = 2 * 1024 * 1024; // 2 MiB

// ── Negotiation types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VideoCodec {
    Vp8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioCodec {
    Opus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoParams {
    pub codec: VideoCodec,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioParams {
    pub codec: AudioCodec,
    /// Samples per second (use 48000 for Opus).
    pub sample_rate: u32,
    pub channels: u8,
}

/// Exchanged once per side on the control stream right after the connection is
/// established. Describes what this side will be sending.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub video: VideoParams,
    pub audio: AudioParams,
}

// ── Media stream types ───────────────────────────────────────────────────────

/// First byte written on every outgoing unidirectional stream so the acceptor
/// can route it to the right receiver. QUIC streams are invisible to the peer
/// until the first byte is sent, so this byte also "announces" the stream.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video = 1,
    Audio = 2,
}

impl MediaKind {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Video),
            2 => Some(Self::Audio),
            _ => None,
        }
    }
}

/// One encoded media frame. `data` is an opaque codec payload (VP8 or Opus
/// bytes, exactly as produced by the WebCodecs `VideoEncoder`/`AudioEncoder`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFrame {
    /// Sender-side capture timestamp in microseconds.
    pub timestamp_us: u64,
    /// True if this is a keyframe / intra-frame. Always false for audio.
    pub is_key: bool,
    pub data: Vec<u8>,
}

// ── Control messages ─────────────────────────────────────────────────────────

/// Sent on the control stream after the initial Hello.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMessage {
    /// The receiver's video decoder is lost; sender must emit a keyframe next.
    KeyframeRequest,
    /// Clean hangup — the sender is closing the call.
    Bye,
}

// ── Framing helpers ──────────────────────────────────────────────────────────
//
// Wire format: u32-LE length prefix followed by postcard-encoded bytes.
// Used for Hello, ControlMessage, and MediaFrame alike.

pub async fn write_msg<T: Serialize>(stream: &mut SendStream, msg: &T) -> anyhow::Result<()> {
    let encoded = postcard::to_stdvec(msg)?;
    let len = encoded.len() as u32;
    anyhow::ensure!(
        len <= MAX_MESSAGE_SIZE,
        "outgoing message too large: {len} bytes (max {MAX_MESSAGE_SIZE})"
    );
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&encoded).await?;
    Ok(())
}

/// Returns `Ok(None)` when the peer finished/closed the stream cleanly at a
/// message boundary. Returns `Err` for connection errors or corrupt framing.
pub async fn read_msg<T: serde::de::DeserializeOwned>(
    stream: &mut RecvStream,
) -> anyhow::Result<Option<T>> {
    let mut len_buf = [0u8; 4];
    // A clean EOF at the start of a new message is the normal way a stream ends.
    // Any error here is treated as end-of-stream so the caller can tear down.
    if stream.read_exact(&mut len_buf).await.is_err() {
        return Ok(None);
    }
    let len = u32::from_le_bytes(len_buf);
    anyhow::ensure!(
        len <= MAX_MESSAGE_SIZE,
        "incoming message too large: {len} bytes (max {MAX_MESSAGE_SIZE})"
    );
    let mut buf = vec![0u8; len as usize];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| anyhow::anyhow!("read body: {e}"))?;
    postcard::from_bytes(&buf).map_err(|e| anyhow::anyhow!("deserialize: {e}")).map(Some)
}
