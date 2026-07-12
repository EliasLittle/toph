import init, { TophSession } from './wasm/toph_wasm.js';
import { startCapture } from './capture.js';
import { startPlayback } from './playback.js';

// ── UI refs ───────────────────────────────────────────────────────────────────

const myTicketEl   = document.getElementById('my-ticket');
const peerTicketEl = document.getElementById('peer-ticket');
const connectBtn   = document.getElementById('connect-btn');
const copyBtn      = document.getElementById('copy-btn');
const statusEl     = document.getElementById('status');
const remoteCanvas = document.getElementById('remote-canvas');
const localVideo   = document.getElementById('local-video');
const logEl        = document.getElementById('log');

function setStatus(msg) {
  statusEl.textContent = msg;
  log(msg);
}

function log(msg) {
  const line = document.createElement('div');
  line.textContent = `${new Date().toLocaleTimeString()} ${msg}`;
  logEl.appendChild(line);
  logEl.scrollTop = logEl.scrollHeight;
}

// ── Session ───────────────────────────────────────────────────────────────────

let session = null;
let activeCall = null;

async function startup() {
  setStatus('Loading WASM…');
  await init();

  setStatus('Binding iroh endpoint…');
  session = await TophSession.create();

  const ticket = await session.ticket();
  myTicketEl.value = ticket;
  connectBtn.disabled = false;
  setStatus('Ready — copy your ticket and share it, or paste a peer ticket to call.');

  armAccept();
}

async function armAccept() {
  log('Waiting for incoming call…');
  try {
    const call = await session.accept(640, 480);
    await onCallEstablished(call);
  } catch (e) {
    setStatus(`Accept error: ${e.message}`);
  }
}

// ── Call setup ────────────────────────────────────────────────────────────────

async function onCallEstablished(call) {
  activeCall = call;
  setStatus(`In call — remote ${call.remote_width()}×${call.remote_height()}`);

  // Wire decoded remote media → canvas + speakers.
  startPlayback(call, remoteCanvas);

  let captureStream = null;

  call.on_close(() => {
    setStatus('Call ended.');
    activeCall = null;
    if (captureStream) {
      captureStream.getTracks().forEach(t => t.stop());
      captureStream = null;
      localVideo.srcObject = null;
    }
    // Clear the remote canvas.
    remoteCanvas.getContext('2d').clearRect(0, 0, remoteCanvas.width, remoteCanvas.height);
    // Re-arm so a new call can come in without a page reload.
    armAccept();
  });

  // Start camera + mic → VP8 / Opus → call.send_video / send_audio.
  // This also registers call.on_keyframe_request so the encoder responds
  // when the remote decoder needs a fresh keyframe.
  try {
    captureStream = await startCapture(call, localVideo);
    log('Camera and microphone active.');
  } catch (e) {
    log(`Capture unavailable: ${e.message} (remote video still displays)`);
  }
}

// ── UI handlers ───────────────────────────────────────────────────────────────

connectBtn.addEventListener('click', async () => {
  const ticket = peerTicketEl.value.trim();
  if (!ticket) return;

  connectBtn.disabled = true;
  setStatus('Connecting…');
  try {
    const call = await session.connect(ticket, 640, 480);
    await onCallEstablished(call);
  } catch (e) {
    setStatus(`Connect failed: ${e.message}`);
    connectBtn.disabled = false;
  }
});

copyBtn.addEventListener('click', () => {
  navigator.clipboard.writeText(myTicketEl.value).then(() => {
    copyBtn.textContent = 'Copied!';
    setTimeout(() => { copyBtn.textContent = 'Copy'; }, 1500);
  });
});

startup().catch(e => setStatus(`Fatal: ${e.message}`));
