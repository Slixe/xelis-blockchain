use crate::core::reader::ReaderError;
use crate::crypto::hash::Hash;
use tokio::sync::mpsc::error::SendError as TSendError;
use tokio::sync::oneshot::error::RecvError;
use std::array::TryFromSliceError;
use std::net::AddrParseError;
use tokio::time::error::Elapsed;
use std::sync::mpsc::SendError;
use std::io::Error as IOError;
use std::sync::PoisonError;
use thiserror::Error;

use super::packet::object::ObjectRequest;

#[derive(Error, Debug)]
pub enum P2pError {
    #[error("Peer disconnected")]
    Disconnected,
    #[error("Invalid handshake")]
    InvalidHandshake,
    #[error("Expected Handshake packet")]
    ExpectedHandshake,
    #[error("Invalid peer address, {}", _0)]
    InvalidPeerAddress(String), // peer address from handshake
    #[error("Invalid network ID")]
    InvalidNetworkID,
    #[error("Peer id {} is already used!", _0)]
    PeerIdAlreadyUsed(u64),
    #[error("Peer already connected: {}", _0)]
    PeerAlreadyConnected(String),
    #[error(transparent)]
    ErrorStd(#[from] IOError),
    #[error("Poison Error: {}", _0)]
    PoisonError(String),
    #[error("Send Error: {}", _0)]
    SendError(String),
    #[error(transparent)]
    TryInto(#[from] TryFromSliceError),
    #[error(transparent)]
    ReaderError(#[from] ReaderError),
    #[error(transparent)]
    ParseAddressError(#[from] AddrParseError),
    #[error("Invalid packet ID")]
    InvalidPacket,
    #[error("Packet size exceed limit")]
    InvalidPacketSize,
    #[error("Received valid packet with not used bytes")]
    InvalidPacketNotFullRead,
    #[error("Request sync chain too fast")]
    RequestSyncChainTooFast,
    #[error(transparent)]
    AsyncTimeOut(#[from] Elapsed),
    #[error("Object requested {:?} not found", _0)]
    ObjectNotFound(ObjectRequest),
    #[error("Object requested {:?} already requested", _0)]
    ObjectAlreadyRequested(ObjectRequest),
    #[error("Invalid object response for request: {:?}, received hash: {}", _0, _1)]
    InvalidObjectResponse(ObjectRequest, Hash),
    #[error(transparent)]
    ObjectRequestError(#[from] RecvError),
    #[error("Expected a block type")]
    ExpectedBlock,
    #[error("Peer sent us a peerlist faster than protocol rules")]
    PeerInvalidPeerListCountdown,
    #[error("Peer sent us a ping packet faster than protocol rules")]
    PeerInvalidPingCoutdown
}

impl<T> From<PoisonError<T>> for P2pError {
    fn from(err: PoisonError<T>) -> Self {
        Self::PoisonError(format!("{}", err))
    }
}

impl<T> From<SendError<T>> for P2pError {
    fn from(err: SendError<T>) -> Self {
        Self::SendError(format!("{}", err))
    }
}

impl<T> From<TSendError<T>> for P2pError {
    fn from(err: TSendError<T>) -> Self {
        Self::SendError(format!("{}", err))
    }
}