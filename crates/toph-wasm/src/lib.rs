use std::cell::RefCell;
use std::rc::Rc;

use futures::channel::mpsc;
use futures::StreamExt;
use js_sys::Function;
use toph_proto::{
    AudioCodec, AudioParams, ConnectionType, ControlMessage, Hello, IncomingCall, MediaFrame,
    Session, VideoCodec, VideoParams,
};
use iroh_base::EndpointId;
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

fn fire0(cb: &Cb) {
    if let Some(f) = cb.borrow().as_ref() {
        let _ = f.call0(&JsValue::NULL);
    }
}

fn fire2(cb: &Cb, a1: &JsValue, a2: &JsValue) {
    if let Some(f) = cb.borrow().as_ref() {
        let _ = f.call2(&JsValue::NULL, a1, a2);
    }
}

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
    pub async fn create() -> Result<TophSession, JsError> {
        let session = Session::spawn().await.map_err(to_js_err)?;
        Ok(TophSession(session))
    }

    /// Returns the 64-char hex node ID. Share this with a peer so they can dial you.
    pub async fn ticket(&self) -> Result<String, JsError> {
        self.0.ticket().await.map_err(to_js_err)
    }

    /// Dial the peer identified by a 64-char hex ticket.
    /// Sends a Ring and waits for Accept/Reject.
    /// Returns `TophCall` on accept, `null` if the remote rejected.
    pub async fn dial(
        &self,
        ticket: String,
        width: u16,
        height: u16,
    ) -> Result<Option<TophCall>, JsError> {
        let result = self
            .0
            .dial(&ticket, local_hello(width, height))
            .await
            .map_err(to_js_err)?;
        Ok(result.map(TophCall::new))
    }

    /// Returns "direct", "relay", or "unknown" for the active path to `node_id_hex`.
    pub async fn connection_type(&self, node_id_hex: String) -> String {
        use std::str::FromStr;
        let Ok(id) = EndpointId::from_str(&node_id_hex) else {
            return "unknown".into();
        };
        match self.0.connection_type(id).await {
            Some(ConnectionType::Direct) => "direct".into(),
            Some(ConnectionType::Relay) => "relay".into(),
            None => "unknown".into(),
        }
    }

    /// Wait for the next incoming connection and return an `IncomingCall`
    /// that the user can accept or reject.
    pub async fn wait_for_ring(&self) -> Result<TophIncomingCall, JsError> {
        let incoming = self.0.wait_for_ring().await.map_err(to_js_err)?;
        Ok(TophIncomingCall(Some(incoming)))
    }
}

// ── TophIncomingCall ──────────────────────────────────────────────────────────

/// A pending incoming call. Call `accept` or `reject` exactly once.
#[wasm_bindgen]
pub struct TophIncomingCall(Option<IncomingCall>);

#[wasm_bindgen]
impl TophIncomingCall {
    /// Accept the call and perform the media handshake.
    /// `width`/`height` are the dimensions of the video *we* will send.
    pub async fn accept(
        &mut self,
        width: u16,
        height: u16,
    ) -> Result<TophCall, JsError> {
        let incoming = self.0.take().ok_or_else(|| JsError::new("already used"))?;
        let call = incoming
            .accept(local_hello(width, height))
            .await
            .map_err(to_js_err)?;
        Ok(TophCall::new(call))
    }

    /// Reject the call.
    pub async fn reject(&mut self) -> Result<(), JsError> {
        let incoming = self.0.take().ok_or_else(|| JsError::new("already used"))?;
        incoming.reject().await.map_err(to_js_err)
    }
}

// ── TophCall ──────────────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct TophCall {
    video_tx: mpsc::Sender<MediaFrame>,
    audio_tx: mpsc::Sender<MediaFrame>,
    control_tx: mpsc::Sender<ControlMessage>,

    remote_width: u16,
    remote_height: u16,
    remote_node_id: String,

    on_video_cb: Cb,
    on_audio_cb: Cb,
    on_keyframe_request_cb: Cb,
    on_close_cb: Cb,
}

#[wasm_bindgen]
impl TophCall {
    pub fn remote_width(&self) -> u16 { self.remote_width }
    pub fn remote_height(&self) -> u16 { self.remote_height }
    pub fn remote_node_id(&self) -> String { self.remote_node_id.clone() }

    pub fn send_video(&mut self, data: &[u8], timestamp_us: f64, is_key: bool) {
        let _ = self.video_tx.try_send(MediaFrame {
            timestamp_us: timestamp_us as u64,
            is_key,
            data: data.to_vec(),
        });
    }

    pub fn send_audio(&mut self, data: &[u8], timestamp_us: f64) {
        let _ = self.audio_tx.try_send(MediaFrame {
            timestamp_us: timestamp_us as u64,
            is_key: false,
            data: data.to_vec(),
        });
    }

    pub fn request_keyframe(&mut self) {
        let _ = self.control_tx.try_send(ControlMessage::KeyframeRequest);
    }

    pub fn hang_up(&mut self) {
        let _ = self.control_tx.try_send(ControlMessage::Bye);
        self.video_tx.close_channel();
        self.audio_tx.close_channel();
        self.control_tx.close_channel();
    }

    /// `callback(data: Uint8Array, timestampUs: number, isKey: boolean)`
    pub fn on_video(&self, cb: Function) {
        *self.on_video_cb.borrow_mut() = Some(cb);
    }

    /// `callback(data: Uint8Array, timestampUs: number)`
    pub fn on_audio(&self, cb: Function) {
        *self.on_audio_cb.borrow_mut() = Some(cb);
    }

    /// `callback()`
    pub fn on_keyframe_request(&self, cb: Function) {
        *self.on_keyframe_request_cb.borrow_mut() = Some(cb);
    }

    /// `callback()` — fires at most once when the call ends for any reason.
    pub fn on_close(&self, cb: Function) {
        *self.on_close_cb.borrow_mut() = Some(cb);
    }
}

impl TophCall {
    fn new(call: toph_proto::Call) -> Self {
        let toph_proto::Call { remote_hello, remote_id, send, recv } = call;
        let toph_proto::CallRecv {
            video: video_recv,
            audio: audio_recv,
            control: control_recv,
        } = recv;

        let (video_tx, video_rx) = mpsc::channel::<MediaFrame>(2);
        let (audio_tx, audio_rx) = mpsc::channel::<MediaFrame>(8);
        let (control_tx, control_rx) = mpsc::channel::<ControlMessage>(4);

        let on_video_cb = new_cb();
        let on_audio_cb = new_cb();
        let on_keyframe_request_cb = new_cb();
        let on_close_cb = new_cb();

        // Write loops: JS channel → QUIC stream.
        spawn_local({
            let mut video_send = send.video;
            let mut rx = video_rx;
            async move {
                while let Some(frame) = rx.next().await {
                    if video_send.send(&frame).await.is_err() { break; }
                }
            }
        });

        spawn_local({
            let mut audio_send = send.audio;
            let mut rx = audio_rx;
            async move {
                while let Some(frame) = rx.next().await {
                    if audio_send.send(&frame).await.is_err() { break; }
                }
            }
        });

        spawn_local({
            let mut control_send = send.control;
            let mut rx = control_rx;
            async move {
                while let Some(msg) = rx.next().await {
                    if control_send.send(&msg).await.is_err() { break; }
                }
            }
        });

        // Read loops: QUIC stream → JS callback.
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
                        _ => { fire_once(&on_close_cb); break; }
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
                        _ => { fire_once(&on_close_cb); break; }
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
                        _ => { fire_once(&on_close_cb); break; }
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
            remote_node_id: remote_id.to_string(),
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
