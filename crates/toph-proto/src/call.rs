use anyhow::Context;
use iroh::endpoint::{Connection, RecvStream, SendStream};

use crate::protocol::{
    read_msg, write_msg, ControlMessage, Hello, MediaFrame, MediaKind,
};
use crate::Result;

// ── Typed stream wrappers ────────────────────────────────────────────────────
//
// Each wrapper pairs the raw QUIC stream with the message type it carries.
// This prevents accidentally reading video from the audio stream, etc.

pub struct VideoSendStream(pub(crate) SendStream);
pub struct AudioSendStream(pub(crate) SendStream);
pub struct ControlSendStream(pub(crate) SendStream);

pub struct VideoRecvStream(pub(crate) RecvStream);
pub struct AudioRecvStream(pub(crate) RecvStream);
pub struct ControlRecvStream(pub(crate) RecvStream);

impl VideoSendStream {
    pub async fn send(&mut self, frame: &MediaFrame) -> Result<()> {
        write_msg(&mut self.0, frame).await
    }
}

impl AudioSendStream {
    pub async fn send(&mut self, frame: &MediaFrame) -> Result<()> {
        write_msg(&mut self.0, frame).await
    }
}

impl ControlSendStream {
    pub async fn send(&mut self, msg: &ControlMessage) -> Result<()> {
        write_msg(&mut self.0, msg).await
    }

    /// Convenience wrapper used by the receiver side when it needs a new keyframe.
    pub async fn request_keyframe(&mut self) -> Result<()> {
        self.send(&ControlMessage::KeyframeRequest).await
    }
}

impl VideoRecvStream {
    /// Returns `Ok(None)` when the peer finished sending (call ended cleanly).
    pub async fn recv(&mut self) -> Result<Option<MediaFrame>> {
        read_msg(&mut self.0).await
    }
}

impl AudioRecvStream {
    pub async fn recv(&mut self) -> Result<Option<MediaFrame>> {
        read_msg(&mut self.0).await
    }
}

impl ControlRecvStream {
    pub async fn recv(&mut self) -> Result<Option<ControlMessage>> {
        read_msg(&mut self.0).await
    }
}

// ── Incoming enum ────────────────────────────────────────────────────────────

/// Returned by the WASM recv loops / tests to distinguish what arrived.
#[derive(Debug)]
pub enum Incoming {
    Video(MediaFrame),
    Audio(MediaFrame),
    Control(ControlMessage),
}

// ── Call ─────────────────────────────────────────────────────────────────────

/// A live peer-to-peer call. Split into `send` and `recv` halves so
/// the WASM layer can move each half into a separate `spawn_local` loop.
pub struct Call {
    /// The `Hello` we received from the remote peer.
    pub remote_hello: Hello,
    pub send: CallSend,
    pub recv: CallRecv,
}

pub struct CallSend {
    pub video: VideoSendStream,
    pub audio: AudioSendStream,
    /// Send `KeyframeRequest` or `Bye` to the remote.
    pub control: ControlSendStream,
}

pub struct CallRecv {
    pub video: VideoRecvStream,
    pub audio: AudioRecvStream,
    pub control: ControlRecvStream,
}

// ── Handshake ────────────────────────────────────────────────────────────────
//
// Sequence (must be followed exactly to avoid deadlocks):
//
//   Connector                          Acceptor
//   ─────────                          ────────
//   open_bi() → (ctl_send, ctl_recv)   accept_bi() → (ctl_recv, ctl_send)
//   write Hello on ctl_send ────────►  read Hello from ctl_recv
//   read Hello from ctl_recv ◄───────  write Hello on ctl_send
//
//   Both sides concurrently after Hello exchange:
//     open_uni() → video send, write [1]
//     open_uni() → audio send, write [2]
//     accept_uni() × 2, read 1 byte each → route to video/audio recv

pub(crate) async fn handshake_connector(conn: Connection, local_hello: Hello) -> Result<Call> {
    // Step 1: open the control bidi stream.
    // The connector must open it (acceptor uses accept_bi).
    let (mut ctl_send, mut ctl_recv) = conn
        .open_bi()
        .await
        .context("open control stream")?;

    // Step 2: send our Hello first — acceptor is blocked on accept_bi until
    // we do this, so we must not wait for theirs first.
    write_msg(&mut ctl_send, &local_hello)
        .await
        .context("send Hello")?;

    // Step 3: read the acceptor's Hello.
    let remote_hello: Hello = read_msg(&mut ctl_recv)
        .await
        .context("read remote Hello")?
        .context("remote closed before sending Hello")?;

    // Step 4: set up media streams concurrently.
    let (video_send, audio_send, video_recv, audio_recv) =
        setup_media_streams(&conn).await?;

    Ok(Call {
        remote_hello,
        send: CallSend {
            video: VideoSendStream(video_send),
            audio: AudioSendStream(audio_send),
            control: ControlSendStream(ctl_send),
        },
        recv: CallRecv {
            video: VideoRecvStream(video_recv),
            audio: AudioRecvStream(audio_recv),
            control: ControlRecvStream(ctl_recv),
        },
    })
}

pub(crate) async fn handshake_acceptor(conn: Connection, local_hello: Hello) -> Result<Call> {
    // Step 1: wait for the connector to open the control stream.
    // accept_bi() will not return until the connector sends the first byte.
    let (mut ctl_send, mut ctl_recv) = conn
        .accept_bi()
        .await
        .context("accept control stream")?;

    // Step 2: the connector already sent its Hello; read it.
    let remote_hello: Hello = read_msg(&mut ctl_recv)
        .await
        .context("read remote Hello")?
        .context("remote closed before sending Hello")?;

    // Step 3: reply with ours.
    write_msg(&mut ctl_send, &local_hello)
        .await
        .context("send Hello")?;

    // Step 4: set up media streams concurrently.
    let (video_send, audio_send, video_recv, audio_recv) =
        setup_media_streams(&conn).await?;

    Ok(Call {
        remote_hello,
        send: CallSend {
            video: VideoSendStream(video_send),
            audio: AudioSendStream(audio_send),
            control: ControlSendStream(ctl_send),
        },
        recv: CallRecv {
            video: VideoRecvStream(video_recv),
            audio: AudioRecvStream(audio_recv),
            control: ControlRecvStream(ctl_recv),
        },
    })
}

/// Opens our two outgoing uni streams and accepts the peer's two incoming uni
/// streams, all concurrently to avoid deadlock.
async fn setup_media_streams(
    conn: &Connection,
) -> Result<(SendStream, SendStream, RecvStream, RecvStream)> {
    // All four futures borrow `conn` as `&Connection` (shared borrow), so
    // running them concurrently via join! is safe and required to avoid deadlock
    // (both sides open and accept concurrently).
    let (vs_r, as_r, r1_r, r2_r) = futures::join!(
        open_media_uni(conn, MediaKind::Video),
        open_media_uni(conn, MediaKind::Audio),
        accept_media_uni(conn),
        accept_media_uni(conn),
    );
    let (vs, a_s, r1, r2) = (vs_r?, as_r?, r1_r?, r2_r?);

    // Route the two incoming streams by their kind byte.
    let (video_recv, audio_recv) = route_recv_streams(r1, r2)?;

    Ok((vs, a_s, video_recv, audio_recv))
}

/// Opens a unidirectional send stream and writes the MediaKind tag byte.
async fn open_media_uni(conn: &Connection, kind: MediaKind) -> Result<SendStream> {
    let mut s = conn.open_uni().await.context("open_uni")?;
    s.write_all(&[kind as u8]).await.context("write kind byte")?;
    Ok(s)
}

/// Accepts a unidirectional receive stream and reads its MediaKind tag byte.
async fn accept_media_uni(conn: &Connection) -> Result<(MediaKind, RecvStream)> {
    let mut s = conn.accept_uni().await.context("accept_uni")?;
    let mut kind_buf = [0u8; 1];
    s.read_exact(&mut kind_buf)
        .await
        .map_err(|e| anyhow::anyhow!("read kind byte: {e}"))?;
    let kind = MediaKind::from_byte(kind_buf[0])
        .context("unknown media kind byte")?;
    Ok((kind, s))
}

/// Given two (kind, stream) pairs in any order, returns (video_recv, audio_recv).
fn route_recv_streams(
    (k1, s1): (MediaKind, RecvStream),
    (k2, s2): (MediaKind, RecvStream),
) -> Result<(RecvStream, RecvStream)> {
    match (k1, k2) {
        (MediaKind::Video, MediaKind::Audio) => Ok((s1, s2)),
        (MediaKind::Audio, MediaKind::Video) => Ok((s2, s1)),
        _ => anyhow::bail!("peer opened two streams of the same media kind"),
    }
}
