//! RISE Node IPC protocol and types.
//!
//! RISE nodes can expose a Unix-socket for co-located services to read chain
//! state and submit transactions rapidly without paying JSON and network costs.

use std::{path::Path, time::Duration};

use alloy_primitives::{Address, B256, Bytes};
use bincode::{Decode, Encode, config, decode_from_slice, encode_into_std_write};
use bytes::{BufMut, BytesMut};
use futures::{SinkExt, StreamExt};
use tokio::{net::UnixStream, time::timeout};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// Compact transaction receipt for IPC clients.
#[derive(Debug, Encode, Decode)]
pub struct RiseIpcReceipt {
    /// Block number the transaction was included in.
    pub block_number: u64,
    /// Whether the transaction executed successfully.
    pub success: bool,
    /// Gas consumed by this transaction.
    pub gas_used: u64,
    /// Logs emitted by this transaction, in execution order.
    pub logs: Vec<RiseIpcReceiptLog>,
}

/// Compact log shape for IPC clients.
#[derive(Debug, Encode, Decode)]
pub struct RiseIpcReceiptLog {
    /// Log index in the whole block.
    pub log_index: u64,
    /// EVM topics for this log.
    #[bincode(with_serde)]
    pub topics: Vec<B256>,
    /// Raw event data.
    #[bincode(with_serde)]
    pub data: Bytes,
}

/// Request frames accepted over IPC.
#[derive(Debug, Encode, Decode)]
pub enum RiseIpcRequest {
    /// Fetch the current base fee.
    GetBaseFee,
    /// Fetch the address's next pending nonce.
    GetPendingNonce(#[bincode(with_serde)] Address),
    /// Submit a raw transaction and wait for the receipt.
    SubmitRawTx(#[bincode(with_serde)] Bytes),
}

/// Response frames emitted over IPC.
#[derive(Debug, Encode, Decode)]
pub enum RiseIpcResponse {
    /// The current base fee.
    BaseFee(u64),
    /// The current next pending nonce for the requested sender.
    PendingNonce(u64),
    /// The receipt for a successful transaction submission.
    Receipt(RiseIpcReceipt),
    /// A server-side error returned instead of a successful response.
    Error(String),
}

/// Errors produced by the shared IPC client.
#[derive(Debug, thiserror::Error)]
pub enum RiseIpcClientError {
    /// Transport-level failure.
    #[error(transparent)]
    Transport(#[from] RiseIpcTransportError),
    /// The server reported an error.
    #[error("IPC server error: {0}")]
    Server(String),
    /// The server replied with a response variant that does not match the request.
    #[error("unexpected IPC response: {0:?}")]
    UnexpectedResponse(RiseIpcResponse),
}

/// Transport-layer IPC errors.
#[derive(Debug, thiserror::Error)]
pub enum RiseIpcTransportError {
    /// Low-level socket or framing failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The operation did not complete within the configured timeout.
    #[error("IPC operation timed out")]
    Timeout(#[from] tokio::time::error::Elapsed),
    /// The peer closed the socket before completing a request/response.
    #[error("IPC peer closed the connection")]
    Closed,
    /// Failed to encode an outbound frame.
    #[error("failed to encode IPC frame: {0}")]
    Encode(#[from] bincode::error::EncodeError),
    /// Failed to decode an inbound frame.
    #[error("failed to decode IPC frame: {0}")]
    Decode(#[from] bincode::error::DecodeError),
}

/// Framed Unix-socket connection for RISE IPC.
#[derive(Debug)]
pub struct RiseIpcConnection {
    framed: Framed<UnixStream, LengthDelimitedCodec>,
    send_buffer: BytesMut,
}

impl RiseIpcConnection {
    /// Wraps an accepted Unix stream with the shared IPC codec.
    pub fn new(stream: UnixStream) -> Self {
        Self {
            framed: Framed::new(stream, LengthDelimitedCodec::new()),
            send_buffer: BytesMut::new(),
        }
    }

    /// Encodes and sends a frame.
    pub async fn send<T: Encode>(&mut self, value: &T) -> Result<(), RiseIpcTransportError> {
        encode_into_std_write(
            value,
            &mut (&mut self.send_buffer).writer(),
            config::standard(),
        )?;
        self.framed.send(self.send_buffer.split().freeze()).await?;
        Ok(())
    }

    /// Reads the next frame and decodes it as `T`.
    pub async fn receive<T: Decode<()>>(&mut self) -> Result<T, RiseIpcTransportError> {
        let Some(frame) = self.framed.next().await else {
            return Err(RiseIpcTransportError::Closed);
        };
        let frame = frame?;
        let (value, _) = decode_from_slice(&frame, config::standard())?;
        Ok(value)
    }
}

/// Thin typed client for RISE IPC requests.
#[derive(Debug)]
pub struct RiseIpcClient {
    conn: RiseIpcConnection,
    ipc_timeout: Duration,
}

impl RiseIpcClient {
    /// Connects a typed IPC client to the configured IPC socket.
    pub async fn connect(
        socket_path: &Path,
        ipc_timeout: Duration,
    ) -> Result<Self, RiseIpcTransportError> {
        let stream = timeout(ipc_timeout, UnixStream::connect(socket_path)).await??;
        Ok(Self {
            conn: RiseIpcConnection::new(stream),
            ipc_timeout,
        })
    }

    /// Sends a request and reads the next response with a bounded timeout.
    async fn send(
        &mut self,
        request: RiseIpcRequest,
    ) -> Result<RiseIpcResponse, RiseIpcClientError> {
        self.conn.send(&request).await?;
        let resp = timeout(self.ipc_timeout, self.conn.receive())
            .await
            .map_err(RiseIpcTransportError::from)??;
        match resp {
            RiseIpcResponse::Error(err) => Err(RiseIpcClientError::Server(err)),
            resp => Ok(resp),
        }
    }

    /// Fetch the current base fee.
    pub async fn get_base_fee(&mut self) -> Result<u64, RiseIpcClientError> {
        match self.send(RiseIpcRequest::GetBaseFee).await? {
            RiseIpcResponse::BaseFee(fee) => Ok(fee),
            resp => Err(RiseIpcClientError::UnexpectedResponse(resp)),
        }
    }

    /// Fetch the address's next pending nonce.
    pub async fn get_pending_nonce(&mut self, address: Address) -> Result<u64, RiseIpcClientError> {
        match self.send(RiseIpcRequest::GetPendingNonce(address)).await? {
            RiseIpcResponse::PendingNonce(nonce) => Ok(nonce),
            resp => Err(RiseIpcClientError::UnexpectedResponse(resp)),
        }
    }

    /// Submit a raw transaction and wait for the receipt.
    pub async fn submit_raw_tx(
        &mut self,
        raw_tx: Bytes,
    ) -> Result<RiseIpcReceipt, RiseIpcClientError> {
        match self.send(RiseIpcRequest::SubmitRawTx(raw_tx)).await? {
            RiseIpcResponse::Receipt(receipt) => Ok(receipt),
            resp => Err(RiseIpcClientError::UnexpectedResponse(resp)),
        }
    }
}
