use anyhow::Context;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh_base::EndpointId;

use crate::protocol::{
    read_msg, write_msg, ControlMessage, Hello, MediaFrame, MediaKind,
};
use crate::Result;

// ── Typed stream wrappers ────────────────────────────────────────────────────

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

    pub async fn request_keyframe(&mut self) -> Result<()> {
        self.send(&ControlMessage::KeyframeRequest).await
    }
}

impl VideoRecvStream {
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

#[derive(Debug)]
pub enum Incoming {
    Video(MediaFrame),
    Audio(MediaFrame),
    Control(ControlMessage),
}

// ── Call ─────────────────────────────────────────────────────────────────────

pub struct Call {
    pub remote_hello: Hello,
    pub remote_id: EndpointId,
    pub send: CallSend,
    pub recv: CallRecv,
}

pub struct CallSend {
    pub video: VideoSendStream,
    pub audio: AudioSendStream,
    pub control: ControlSendStream,
}

pub struct CallRecv {
    pub video: VideoRecvStream,
    pub audio: AudioRecvStream,
    pub control: ControlRecvStream,
}

// ── Call builders ─────────────────────────────────────────────────────────────
//
// Both functions receive a pre-established control bidi stream. The Ring/Accept
// signal exchange happened before these are called (in session.rs). These
// functions exchange Hello and set up the four media streams.
//
// Hello ordering (connector writes first to avoid deadlock):
//
//   Connector                          Acceptor
//   write Hello → ctl_send ─────────► read Hello ← ctl_recv
//   read Hello  ← ctl_recv ◄───────── write Hello → ctl_send
//
//   Both sides concurrently after Hello:
//     open_uni() → video send, write [1]
//     open_uni() → audio send, write [2]
//     accept_uni() × 2, read tag → route to video/audio recv

pub(crate) async fn build_call_connector(
    conn: Connection,
    mut ctl_send: SendStream,
    mut ctl_recv: RecvStream,
    local_hello: Hello,
) -> Result<Call> {
    let remote_id = conn.remote_id();

    write_msg(&mut ctl_send, &local_hello)
        .await
        .context("send Hello")?;

    let remote_hello: Hello = read_msg(&mut ctl_recv)
        .await
        .context("read remote Hello")?
        .context("remote closed before sending Hello")?;

    let (video_send, audio_send, video_recv, audio_recv) =
        setup_media_streams(&conn).await?;

    Ok(Call {
        remote_hello,
        remote_id,
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

pub(crate) async fn build_call_acceptor(
    conn: Connection,
    mut ctl_send: SendStream,
    mut ctl_recv: RecvStream,
    local_hello: Hello,
) -> Result<Call> {
    let remote_id = conn.remote_id();

    // Connector writes Hello first; read it before replying.
    let remote_hello: Hello = read_msg(&mut ctl_recv)
        .await
        .context("read remote Hello")?
        .context("remote closed before sending Hello")?;

    write_msg(&mut ctl_send, &local_hello)
        .await
        .context("send Hello")?;

    let (video_send, audio_send, video_recv, audio_recv) =
        setup_media_streams(&conn).await?;

    Ok(Call {
        remote_hello,
        remote_id,
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

// ── Media stream helpers ──────────────────────────────────────────────────────

async fn setup_media_streams(
    conn: &Connection,
) -> Result<(SendStream, SendStream, RecvStream, RecvStream)> {
    let (vs_r, as_r, r1_r, r2_r) = futures::join!(
        open_media_uni(conn, MediaKind::Video),
        open_media_uni(conn, MediaKind::Audio),
        accept_media_uni(conn),
        accept_media_uni(conn),
    );
    let (vs, a_s, r1, r2) = (vs_r?, as_r?, r1_r?, r2_r?);
    let (video_recv, audio_recv) = route_recv_streams(r1, r2)?;
    Ok((vs, a_s, video_recv, audio_recv))
}

async fn open_media_uni(conn: &Connection, kind: MediaKind) -> Result<SendStream> {
    let mut s = conn.open_uni().await.context("open_uni")?;
    s.write_all(&[kind as u8]).await.context("write kind byte")?;
    Ok(s)
}

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
