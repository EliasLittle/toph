import init, { TophSession } from './wasm/toph_wasm.js';
import { startCapture } from './capture.js';
import { startPlayback } from './playback.js';

// ── UI refs ───────────────────────────────────────────────────────────────────

const myTicketEl      = document.getElementById('my-ticket');
const peerTicketEl    = document.getElementById('peer-ticket');
const connectBtn      = document.getElementById('connect-btn');
const copyBtn         = document.getElementById('copy-btn');
const statusEl        = document.getElementById('status');
const remoteCanvas    = document.getElementById('remote-canvas');
const localVideo      = document.getElementById('local-video');
const logEl           = document.getElementById('log');
const callControls    = document.getElementById('call-controls');
const muteBtn         = document.getElementById('mute-btn');
const cameraBtn       = document.getElementById('camera-btn');
const endBtn          = document.getElementById('end-btn');
const incomingOverlay = document.getElementById('incoming-overlay');
const acceptBtn       = document.getElementById('accept-btn');
const rejectBtn       = document.getElementById('reject-btn');
const connBadge       = document.getElementById('conn-badge');
const debugToggleBtn  = document.getElementById('debug-toggle');

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

// ── Debug mode ────────────────────────────────────────────────────────────────

let debugMode = false;

debugToggleBtn.addEventListener('click', () => {
  debugMode = !debugMode;
  debugToggleBtn.classList.toggle('active', debugMode);
  debugToggleBtn.textContent = debugMode ? 'Debug ON' : 'Debug';
});

function debugLog(msg) {
  if (!debugMode) return;
  const line = document.createElement('div');
  line.className = 'debug-line';
  line.textContent = `${new Date().toLocaleTimeString()} [debug] ${msg}`;
  logEl.appendChild(line);
  logEl.scrollTop = logEl.scrollHeight;
}

function formatConnDebug(info) {
  const parts = [`path=${info.conn_type}`];
  const relayUrl = info.relay_active[0] ?? info.relay_idle[0];
  if (relayUrl) {
    try { parts.push(`relay=${new URL(relayUrl).hostname}`); } catch (_) {}
  }
  if (info.conn_type !== 'direct') {
    const n = info.direct_idle.length;
    if (n > 0) {
      parts.push(`${n} direct candidate${n > 1 ? 's' : ''} (idle) — hole-punch not established`);
    } else {
      parts.push('no direct candidates — CGNAT or UDP blocked');
    }
  } else {
    if (info.direct_active[0]) parts.push(`via ${info.direct_active[0]}`);
  }
  return parts.join(' | ');
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
  setStatus('Ready — copy your ticket and share it, or paste a peer ticket to dial.');

  armRing();
}

// ── Connection type badge ─────────────────────────────────────────────────────

function setConnBadge(type) {
  connBadge.className = type;
  connBadge.textContent =
    type === 'direct' ? '⬤ Direct' :
    type === 'relay'  ? '⬤ Relay'  : '';
}

function startConnectionPoller(call) {
  let stopped = false;
  const nodeId = call.remote_node_id();

  async function poll() {
    if (stopped) return;
    const type = await session.connection_type(nodeId);
    setConnBadge(type);
    if (debugMode) {
      const raw = await session.connection_debug_info(nodeId);
      if (raw) {
        try { debugLog(formatConnDebug(JSON.parse(raw))); } catch (_) {}
      }
    }
    setTimeout(poll, 3000);
  }

  poll();
  return () => { stopped = true; connBadge.className = ''; connBadge.textContent = ''; };
}

// ── Incoming call loop ────────────────────────────────────────────────────────

async function armRing() {
  log('Listening for incoming calls…');
  try {
    const incoming = await session.wait_for_ring();
    showIncomingCall(incoming);
  } catch (e) {
    setStatus(`Ring listener error: ${e.message}`);
  }
}

function showIncomingCall(incoming) {
  incomingOverlay.classList.add('visible');
  setStatus('Incoming call — accept or reject.');

  acceptBtn.onclick = async () => {
    incomingOverlay.classList.remove('visible');
    connectBtn.disabled = true;
    setStatus('Accepting call…');
    try {
      const call = await incoming.accept(640, 480);
      await onCallEstablished(call);
    } catch (e) {
      setStatus(`Accept failed: ${e.message}`);
      connectBtn.disabled = false;
      armRing();
    }
  };

  rejectBtn.onclick = async () => {
    incomingOverlay.classList.remove('visible');
    setStatus('Call rejected.');
    try { await incoming.reject(); } catch (_) {}
    armRing();
  };
}

// ── Call setup ────────────────────────────────────────────────────────────────

async function onCallEstablished(call) {
  activeCall = call;
  setStatus(`In call — remote ${call.remote_width()}×${call.remote_height()}`);

  let stopPlayback = startPlayback(call, remoteCanvas);

  let captureStream = null;
  const stopPoller = startConnectionPoller(call);

  function teardown() {
    stopPoller();
    if (stopPlayback) { stopPlayback(); stopPlayback = null; }
    callControls.classList.remove('visible');
    connectBtn.disabled = false;
    activeCall = null;
    if (captureStream) {
      captureStream.getTracks().forEach(t => t.stop());
      captureStream = null;
      localVideo.srcObject = null;
      localVideo.load();
    }
    remoteCanvas.getContext('2d').clearRect(0, 0, remoteCanvas.width, remoteCanvas.height);
    localVideo.style.opacity = '1';
    armRing();
  }

  call.on_close(() => {
    setStatus('Call ended.');
    teardown();
  });

  try {
    captureStream = await startCapture(call, localVideo);
    log('Camera and microphone active.');
  } catch (e) {
    log(`Capture unavailable: ${e.message} (remote video still displays)`);
  }

  callControls.classList.add('visible');

  muteBtn.textContent = 'Mute Mic';
  muteBtn.classList.remove('active');
  cameraBtn.textContent = 'Stop Camera';
  cameraBtn.classList.remove('active');

  muteBtn.onclick = () => {
    if (!captureStream) return;
    const track = captureStream.getAudioTracks()[0];
    if (!track) return;
    track.enabled = !track.enabled;
    const muted = !track.enabled;
    muteBtn.textContent = muted ? 'Unmute Mic' : 'Mute Mic';
    muteBtn.classList.toggle('active', muted);
  };

  cameraBtn.onclick = () => {
    if (!captureStream) return;
    const track = captureStream.getVideoTracks()[0];
    if (!track) return;
    track.enabled = !track.enabled;
    const stopped = !track.enabled;
    cameraBtn.textContent = stopped ? 'Start Camera' : 'Stop Camera';
    cameraBtn.classList.toggle('active', stopped);
    localVideo.style.opacity = stopped ? '0.15' : '1';
  };

  endBtn.onclick = () => {
    if (activeCall) activeCall.hang_up();
  };
}

// ── UI handlers ───────────────────────────────────────────────────────────────

connectBtn.addEventListener('click', async () => {
  const ticket = peerTicketEl.value.trim();
  if (!ticket) return;

  connectBtn.disabled = true;
  setStatus('Dialling…');
  try {
    const call = await session.dial(ticket, 640, 480);
    if (call === null || call === undefined) {
      setStatus('Call rejected by peer.');
      connectBtn.disabled = false;
      return;
    }
    await onCallEstablished(call);
  } catch (e) {
    setStatus(`Dial failed: ${e.message}`);
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
