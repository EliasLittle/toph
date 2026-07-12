# toph

A simple peer-to-peer video and audio calling app for the browser.

> **Vibe-coded.** This project was built through a conversation with Claude. The architecture is real and the code works, but treat it as a learning project rather than production software.

Built on [iroh](https://iroh.computer) — a Rust library for direct peer-to-peer connections over QUIC. There is no signaling server, no TURN server, and no backend. Two people open the page, exchange a short ID, and connect directly.

---

## How it works

- **`crates/toph-proto`** — Rust library implementing the wire protocol: QUIC stream framing, a ring/accept/reject handshake, VP8 video frames, and Opus audio frames.
- **`crates/toph-wasm`** — WASM bindings (via `wasm-bindgen`) that expose the protocol to the browser.
- **`web/`** — Vite frontend using the WebCodecs API (`VideoEncoder`, `VideoDecoder`, `AudioEncoder`, `AudioDecoder`) and `MediaStreamTrackProcessor` for camera and microphone capture.

NAT traversal is handled by iroh's relay infrastructure (operated by [number 0](https://n0.computer)). The app shows whether your connection is **Direct** (peer-to-peer UDP) or **Relay** (traffic routed through a relay server) — iroh tries to upgrade to direct in the background.

---

## Requirements

- Rust (stable) with the `wasm32-unknown-unknown` target
- LLVM clang (for compiling `ring` to WASM — Apple clang doesn't support WASM targets)
- `wasm-bindgen-cli` matching the version in `crates/toph-wasm/Cargo.toml`
- Node.js

```sh
# Install LLVM (macOS)
brew install llvm

# Add the WASM target
rustup target add wasm32-unknown-unknown

# Install wasm-bindgen-cli (check Cargo.toml for the exact version)
cargo install wasm-bindgen-cli --version 0.2.126
```

---

## Development

```sh
cd web
npm install
npm run dev        # builds WASM then starts Vite dev server
```

Open two browser tabs at the same URL. Copy the ID from one tab and paste it into the other, then click **Dial**.

---

## Production build

```sh
cd web
npm run build      # builds WASM + Vite production bundle → web/dist/
npm run preview    # serves web/dist/ locally to verify the build
```

The output in `web/dist/` is a fully static site. Drop it on any host that serves HTTPS (Netlify, Cloudflare Pages, Vercel, etc.). No server-side component required.

---

## Call flow

```
Dialer                          Callee
──────                          ──────
connect() ──────────────────►
open control stream
send Ring ──────────────────►   wait_for_ring()
                                show Accept/Reject UI
◄─────────────── send Accept    incoming.accept()
exchange Hello (codec params)
open 2 uni streams (video, audio) each direction
                [ call is live ]
```

Reject closes the connection immediately without opening any media streams.

---

## Project layout

```
toph/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── toph-proto/             # wire protocol (pure Rust, no WASM)
│   │   ├── src/
│   │   │   ├── protocol.rs     # framing, SignalMessage, Hello, MediaFrame
│   │   │   ├── session.rs      # Session, IncomingCall, ConnectionType
│   │   │   └── call.rs         # Call, typed stream wrappers
│   │   └── tests/
│   │       └── roundtrip.rs    # integration test: two endpoints exchange frames
│   └── toph-wasm/              # wasm-bindgen glue
│       └── src/lib.rs          # TophSession, TophIncomingCall, TophCall
└── web/                        # browser frontend
    ├── src/
    │   ├── main.js             # session setup, call lifecycle, UI logic
    │   ├── capture.js          # getUserMedia → VideoEncoder / AudioEncoder
    │   └── playback.js         # VideoDecoder / AudioDecoder → canvas + AudioContext
    └── index.html
```

---

## Running the test

```sh
cargo test -p toph-proto
```

Spins up two in-process iroh endpoints, dials one from the other, exchanges video and audio frames in both directions, and verifies they arrive intact.
