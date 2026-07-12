use std::str::FromStr;

use anyhow::Context;
use iroh_base::{EndpointAddr, EndpointId, TransportAddr};

use crate::{
    call::{build_call_acceptor, build_call_connector},
    protocol::{Hello, SignalMessage, ALPN},
    Call, Result,
};

/// Owns the iroh `Endpoint` for the lifetime of the application.
/// Create exactly one `Session` per process.
pub struct Session {
    endpoint: iroh::Endpoint,
}

impl Session {
    /// Bind an endpoint using n0's public relay infrastructure.
    pub async fn spawn() -> Result<Self> {
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .context("failed to bind iroh endpoint")?;
        Ok(Self { endpoint })
    }

    /// Returns a short 64-char hex node ID. Share this with a peer so they
    /// can dial you. Waits for the relay to come up so the ID is routable.
    pub async fn ticket(&self) -> Result<String> {
        self.endpoint.online().await;
        Ok(self.endpoint.id().to_string())
    }

    /// Dial the peer identified by a 64-char hex node ID ticket.
    /// Sends a Ring and waits for the remote's Accept/Reject.
    /// Returns `Some(Call)` on accept, `None` if the remote rejected.
    pub async fn dial(&self, ticket: &str, local_hello: Hello) -> Result<Option<Call>> {
        let id = EndpointId::from_str(ticket).context("invalid ticket")?;
        let addr = EndpointAddr::from(id);
        let conn = self
            .endpoint
            .connect(addr, ALPN)
            .await
            .context("iroh connect failed")?;

        // Open control stream and send Ring.
        let (mut ctl_send, mut ctl_recv) = conn
            .open_bi()
            .await
            .context("open control stream")?;

        crate::protocol::write_msg(&mut ctl_send, &SignalMessage::Ring)
            .await
            .context("send Ring")?;

        // Wait for Accept or Reject.
        let response: SignalMessage = crate::protocol::read_msg(&mut ctl_recv)
            .await
            .context("read signal response")?
            .context("remote closed without responding")?;

        match response {
            SignalMessage::Accept => {
                let call =
                    build_call_connector(conn, ctl_send, ctl_recv, local_hello).await?;
                Ok(Some(call))
            }
            SignalMessage::Reject => Ok(None),
            SignalMessage::Ring => {
                anyhow::bail!("unexpected Ring from remote (expected Accept or Reject)")
            }
        }
    }

    /// Returns whether the active path to `remote_id` is direct or relay-assisted.
    /// Returns `None` if iroh has no info for that peer (not yet connected, or
    /// no active path).
    pub async fn connection_type(&self, remote_id: EndpointId) -> Option<ConnectionType> {
        let info = self.endpoint.remote_info(remote_id).await?;
        let has_direct = info
            .addrs()
            .any(|a| matches!(a.usage(), iroh::endpoint::TransportAddrUsage::Active)
                && matches!(a.addr(), TransportAddr::Ip(_)));
        Some(if has_direct {
            ConnectionType::Direct
        } else {
            ConnectionType::Relay
        })
    }

    /// Wait for the next incoming connection. Returns an `IncomingCall` that
    /// the caller can accept or reject before any media streams are opened.
    pub async fn wait_for_ring(&self) -> Result<IncomingCall> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .context("endpoint closed")?;

        let conn = incoming
            .await
            .context("connection handshake failed")?;

        // The dialler opens the control stream and sends Ring first.
        let (ctl_send, mut ctl_recv) = conn
            .accept_bi()
            .await
            .context("accept control stream")?;

        let msg: SignalMessage = crate::protocol::read_msg(&mut ctl_recv)
            .await
            .context("read signal")?
            .context("remote closed without ringing")?;

        anyhow::ensure!(
            matches!(msg, SignalMessage::Ring),
            "expected Ring from remote, got {:?}",
            msg
        );

        Ok(IncomingCall { conn, ctl_send, ctl_recv })
    }
}

// ── ConnectionType ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    /// Traffic flows directly between peers over UDP.
    Direct,
    /// Traffic is relayed through iroh's relay server.
    Relay,
}

// ── IncomingCall ──────────────────────────────────────────────────────────────

/// An incoming ring that has not yet been accepted or rejected.
/// Obtained from `Session::wait_for_ring`.
pub struct IncomingCall {
    conn: iroh::endpoint::Connection,
    ctl_send: iroh::endpoint::SendStream,
    ctl_recv: iroh::endpoint::RecvStream,
}

impl IncomingCall {
    /// Accept the call and complete the handshake.
    /// `local_hello` describes the video/audio format this side will send.
    pub async fn accept(mut self, local_hello: Hello) -> Result<Call> {
        crate::protocol::write_msg(&mut self.ctl_send, &SignalMessage::Accept)
            .await
            .context("send Accept")?;
        build_call_acceptor(self.conn, self.ctl_send, self.ctl_recv, local_hello).await
    }

    /// Reject the call. Sends a Reject message and closes the connection.
    pub async fn reject(mut self) -> Result<()> {
        crate::protocol::write_msg(&mut self.ctl_send, &SignalMessage::Reject)
            .await
            .context("send Reject")?;
        self.conn.close(0u32.into(), b"rejected");
        Ok(())
    }
}
