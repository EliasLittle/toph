use std::cell::RefCell;
use std::rc::Rc;

use futures::channel::mpsc;
use futures::StreamExt;
use js_sys::Function;
use toph_proto::{
    AudioCodec, AudioParams, ControlMessage, Hello, MediaFrame, Session, VideoCodec, VideoParams,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

// ── Init ──────────────────────────────────────────────────────────────────────

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

// ── Callback helpers ──────────────────────────────────────────────────────────

type Cb = Rc<RefCell<Option<Function>>>;

fn new_cb() -> Cb {
    Rc::new(RefCell::new(None))
}

/// Invoke a zero-argument callback.
fn fire0(cb: &Cb) {
    if let Some(f) = cb.borrow().as_ref() {
        let _ = f.call0(&JsValue::NULL);
    }
}

/// Invoke a two-argument callback.
fn fire2(cb: &Cb, a1: &JsValue, a2: &JsValue) {
    if let Some(f) = cb.borrow().as_ref() {
        let _ = f.call2(&JsValue::NULL, a1, a2);
    }
}

/// Invoke a three-argument callback.
fn fire3(cb: &Cb, a1: &JsValue, a2: &JsValue, a3: &JsValue) {
    if let Some(f) = cb.borrow().as_ref() {
        let _ = f.call3(&JsValue::NULL, a1, a2, a3);
    }
}

/// Invoke then clear — guarantees the callback fires at most once.
fn fire_once(cb: &Cb) {
    let f = cb.borrow_mut().take();
    if let Some(f) = f {
        let _ = f.call0(&JsValue::NULL);
    }
}

// ── TophSession ───────────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct TophSession(Session);

#[wasm_bindgen]
impl TophSession {
    /// Bind an iroh endpoint and wait for the relay connection to come up.
    /// Create once on page load; reuse the instance for multiple calls.
    pub async fn create() -> Result<TophSession, JsError> {
        let session = Session::spawn().await.map_err(to_js_err)?;
        Ok(TophSession(session))
    }

    /// Returns a JSON-encoded EndpointAddr ticket string. Share this with your
    /// peer so they can connect to you.
    pub async fn ticket(&self) -> Result<String, JsError> {
        self.0.ticket().await.map_err(to_js_err)
    }

    /// Dial the peer described by `ticket` and complete the protocol handshake.
    /// `width`/`height` are the dimensions of the video *we* will send.
    pub async fn connect(
        &self,
        ticket: String,
        width: u16,
        height: u16,
    ) -> Result<TophCall, JsError> {
        let call = self.0.connect(&ticket, local_hello(width, height)).await.map_err(to_js_err)?;
        Ok(TophCall::new(call))
    }

    /// Block until the next incoming connection arrives and the handshake completes.
    /// Call this in the background immediately after `create()` so you're ready
    /// to receive a call while the user shares their ticket.
    pub async fn accept(&self, width: u16, height: u16) -> Result<TophCall, JsError> {
        let call = self.0.accept(local_hello(width, height)).await.map_err(to_js_err)?;
        Ok(TophCall::new(call))
    }
}

// ── TophCall ──────────────────────────────────────────────────────────────────

/// An active call. Background loops (started at construction) drive the QUIC
/// streams; JS interacts entirely through the synchronous send methods and the
/// registered callbacks.
#[wasm_bindgen]
pub struct TophCall {
    // Bounded channels from JS sends → background QUIC write loops.
    // `try_send` drops frames silently on backpressure (stale frames are
    // worthless in real-time; the decoder will request a keyframe if needed).
    video_tx: mpsc::Sender<MediaFrame>,
    audio_tx: mpsc::Sender<MediaFrame>,
    control_tx: mpsc::Sender<ControlMessage>,

    remote_width: u16,
    remote_height: u16,

    // Callbacks set by JS after construction.
    on_video_cb: Cb,
    on_audio_cb: Cb,
    on_keyframe_request_cb: Cb,
    on_close_cb: Cb,
}

#[wasm_bindgen]
impl TophCall {
    /// Width of the video the remote peer will send us.
    pub fn remote_width(&self) -> u16 {
        self.remote_width
    }

    /// Height of the video the remote peer will send us.
    pub fn remote_height(&self) -> u16 {
        self.remote_height
    }

    // ── Send ─────────────────────────────────────────────────────────────────

    /// Push an encoded VP8 chunk from the VideoEncoder output callback.
    /// Synchronous: copies `data` and returns immediately.
    /// Frames are silently dropped if the outbound channel is full (backpressure).
    pub fn send_video(&mut self, data: &[u8], timestamp_us: f64, is_key: bool) {
        let _ = self.video_tx.try_send(MediaFrame {
            timestamp_us: timestamp_us as u64,
            is_key,
            data: data.to_vec(),
        });
    }

    /// Push an encoded Opus chunk from the AudioEncoder output callback.
    pub fn send_audio(&mut self, data: &[u8], timestamp_us: f64) {
        let _ = self.audio_tx.try_send(MediaFrame {
            timestamp_us: timestamp_us as u64,
            is_key: false,
            data: data.to_vec(),
        });
    }

    /// Ask the remote to send a keyframe on their next encode (call after a
    /// VideoDecoder error or when starting to display after a gap).
    pub fn request_keyframe(&mut self) {
        let _ = self.control_tx.try_send(ControlMessage::KeyframeRequest);
    }

    /// Signal the remote that this side is hanging up, then close the call.
    pub fn hang_up(&mut self) {
        let _ = self.control_tx.try_send(ControlMessage::Bye);
        // Dropping the senders closes the write loops, which finishes the
        // QUIC send streams, which signals end-of-stream to the peer.
        self.video_tx.close_channel();
        self.audio_tx.close_channel();
        self.control_tx.close_channel();
    }

    // ── Callbacks ────────────────────────────────────────────────────────────

    /// `callback(data: Uint8Array, timestampUs: number, isKey: boolean)`
    /// Called on each incoming VP8 frame from the remote.
    pub fn on_video(&self, cb: Function) {
        *self.on_video_cb.borrow_mut() = Some(cb);
    }

    /// `callback(data: Uint8Array, timestampUs: number)`
    /// Called on each incoming Opus frame from the remote.
    pub fn on_audio(&self, cb: Function) {
        *self.on_audio_cb.borrow_mut() = Some(cb);
    }

    /// `callback()`
    /// Called when the remote asks us to emit a keyframe. Wire this to set a
    /// flag that forces `keyFrame: true` on the next `VideoEncoder.encode()`.
    pub fn on_keyframe_request(&self, cb: Function) {
        *self.on_keyframe_request_cb.borrow_mut() = Some(cb);
    }

    /// `callback()`
    /// Called once when the call ends for any reason (remote hung up, connection
    /// lost). Guaranteed to fire at most once regardless of which stream closes first.
    pub fn on_close(&self, cb: Function) {
        *self.on_close_cb.borrow_mut() = Some(cb);
    }
}

impl TophCall {
    fn new(call: toph_proto::Call) -> Self {
        let toph_proto::Call { remote_hello, send, recv } = call;
        let toph_proto::CallRecv {
            video: video_recv,
            audio: audio_recv,
            control: control_recv,
        } = recv;

        // Bounded: 2 video frames (stale = useless), 8 audio (small & gapless)
        let (video_tx, video_rx) = mpsc::channel::<MediaFrame>(2);
        let (audio_tx, audio_rx) = mpsc::channel::<MediaFrame>(8);
        let (control_tx, control_rx) = mpsc::channel::<ControlMessage>(4);

        let on_video_cb = new_cb();
        let on_audio_cb = new_cb();
        let on_keyframe_request_cb = new_cb();
        let on_close_cb = new_cb();

        // ── Write loops: JS channel → QUIC stream ────────────────────────────

        spawn_local({
            let mut video_send = send.video;
            let mut rx = video_rx;
            async move {
                while let Some(frame) = rx.next().await {
                    if video_send.send(&frame).await.is_err() {
                        break;
                    }
                }
            }
        });

        spawn_local({
            let mut audio_send = send.audio;
            let mut rx = audio_rx;
            async move {
                while let Some(frame) = rx.next().await {
                    if audio_send.send(&frame).await.is_err() {
                        break;
                    }
                }
            }
        });

        spawn_local({
            let mut control_send = send.control;
            let mut rx = control_rx;
            async move {
                while let Some(msg) = rx.next().await {
                    if control_send.send(&msg).await.is_err() {
                        break;
                    }
                }
            }
        });

        // ── Read loops: QUIC stream → JS callback ────────────────────────────

        spawn_local({
            let on_video_cb = on_video_cb.clone();
            let on_close_cb = on_close_cb.clone();
            let mut video_recv = video_recv;
            async move {
                loop {
                    match video_recv.recv().await {
                        Ok(Some(frame)) => {
                            let arr = js_sys::Uint8Array::new_with_length(frame.data.len() as u32);
                            arr.copy_from(&frame.data);
                            fire3(
                                &on_video_cb,
                                &arr,
                                &JsValue::from_f64(frame.timestamp_us as f64),
                                &JsValue::from_bool(frame.is_key),
                            );
                        }
                        _ => {
                            fire_once(&on_close_cb);
                            break;
                        }
                    }
                }
            }
        });

        spawn_local({
            let on_audio_cb = on_audio_cb.clone();
            let on_close_cb = on_close_cb.clone();
            let mut audio_recv = audio_recv;
            async move {
                loop {
                    match audio_recv.recv().await {
                        Ok(Some(frame)) => {
                            let arr = js_sys::Uint8Array::new_with_length(frame.data.len() as u32);
                            arr.copy_from(&frame.data);
                            fire2(
                                &on_audio_cb,
                                &arr,
                                &JsValue::from_f64(frame.timestamp_us as f64),
                            );
                        }
                        _ => {
                            fire_once(&on_close_cb);
                            break;
                        }
                    }
                }
            }
        });

        spawn_local({
            let on_keyframe_request_cb = on_keyframe_request_cb.clone();
            let on_close_cb = on_close_cb.clone();
            let mut control_recv = control_recv;
            async move {
                loop {
                    match control_recv.recv().await {
                        Ok(Some(ControlMessage::KeyframeRequest)) => {
                            fire0(&on_keyframe_request_cb);
                        }
                        // Bye, stream closed, or error → call ended
                        _ => {
                            fire_once(&on_close_cb);
                            break;
                        }
                    }
                }
            }
        });

        TophCall {
            video_tx,
            audio_tx,
            control_tx,
            remote_width: remote_hello.video.width,
            remote_height: remote_hello.video.height,
            on_video_cb,
            on_audio_cb,
            on_keyframe_request_cb,
            on_close_cb,
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn local_hello(width: u16, height: u16) -> Hello {
    Hello {
        video: VideoParams { codec: VideoCodec::Vp8, width, height },
        audio: AudioParams { codec: AudioCodec::Opus, sample_rate: 48000, channels: 1 },
    }
}

fn to_js_err(e: anyhow::Error) -> JsError {
    JsError::new(&e.to_string())
}
