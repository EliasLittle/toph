use anyhow::Context;
use iroh_base::EndpointAddr;

use crate::{
    call::{handshake_acceptor, handshake_connector},
    protocol::{Hello, ALPN},
    Call, Result,
};

/// Owns the iroh `Endpoint` for the lifetime of the application.
/// Create exactly one `Session` per process.
pub struct Session {
    endpoint: iroh::Endpoint,
}

impl Session {
    /// Bind an endpoint using n0's public relay infrastructure.
    /// Registers our ALPN so incoming connections are routed to `accept`.
    pub async fn spawn() -> Result<Self> {
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .context("failed to bind iroh endpoint")?;
        Ok(Self { endpoint })
    }

    /// Returns a JSON-encoded `EndpointAddr` ticket string suitable for
    /// copy-pasting to a peer. Waits for the relay connection to be
    /// established first so the ticket contains a usable relay hint.
    ///
    /// # Note
    /// The exact iroh 1.0 API for waiting until online may need to be
    /// verified against docs.rs/iroh. `online()` is the expected method name.
    pub async fn ticket(&self) -> Result<String> {
        self.endpoint.online().await;
        let addr = self.endpoint.addr();
        serde_json::to_string(&addr).context("failed to serialize endpoint addr")
    }

    /// Dial the peer described by `ticket` and perform the protocol handshake.
    /// `local_hello` describes the video/audio format this side will send.
    pub async fn connect(&self, ticket: &str, local_hello: Hello) -> Result<Call> {
        let addr: EndpointAddr =
            serde_json::from_str(ticket).context("failed to parse ticket")?;
        let conn = self
            .endpoint
            .connect(addr, ALPN)
            .await
            .context("iroh connect failed")?;
        handshake_connector(conn, local_hello).await
    }

    /// Accept the next incoming connection and perform the protocol handshake.
    /// `local_hello` describes the video/audio format this side will send.
    ///
    /// Typical usage: call this in a loop (or once for a 1:1 call) after
    /// sharing your ticket with a peer.
    pub async fn accept(&self, local_hello: Hello) -> Result<Call> {
        // endpoint.accept() -> Accept<'_>: Output = Option<Incoming>.
        // Incoming implements IntoFuture -> Result<Connection, ConnectingError>.
        let incoming = self
            .endpoint
            .accept()
            .await
            .context("endpoint closed before any connection arrived")?;

        let conn = incoming
            .await
            .context("connection handshake failed")?;

        handshake_acceptor(conn, local_hello).await
    }
}
