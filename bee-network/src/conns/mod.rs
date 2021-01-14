// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

mod dial;
mod errors;
mod manager;

pub use dial::*;
pub use errors::Error;
pub use manager::*;

use crate::{
    interaction::events::{InternalEvent, InternalEventSender},
    peers::{self, DataReceiver, PeerInfo},
    protocols::gossip::{GossipProtocol, GossipSubstream},
    PeerId, ShortId,
};

use futures::{prelude::*, select, AsyncRead, AsyncWrite};
use libp2p::core::{
    muxing::{event_from_ref_and_wrap, outbound_from_ref_and_wrap, StreamMuxerBox},
    upgrade,
};
use log::*;
use tokio::task::JoinHandle;

use std::{fmt, sync::Arc};

pub(crate) async fn upgrade_connection(
    peer_id: PeerId,
    peer_info: PeerInfo,
    muxer: StreamMuxerBox,
    origin: Origin,
    internal_event_sender: InternalEventSender,
) -> Result<(), Error> {
    let muxer = Arc::new(muxer);
    let (message_sender, message_receiver) = peers::channel();

    let internal_event_sender_clone = internal_event_sender.clone();

    let substream = match origin {
        Origin::Outbound => {
            let outbound = outbound_from_ref_and_wrap(muxer)
                .fuse()
                .await
                .map_err(|_| Error::CreatingOutboundSubstreamFailed(peer_id.short()))?;

            upgrade::apply_outbound(outbound, GossipProtocol, upgrade::Version::V1)
                .await
                .map_err(|_| Error::SubstreamProtocolUpgradeFailed(peer_id.short()))?
        }
        Origin::Inbound => {
            let inbound = loop {
                if let Some(inbound) = event_from_ref_and_wrap(muxer.clone())
                    .await
                    .map_err(|_| Error::CreatingInboundSubstreamFailed(peer_id.short()))?
                    .into_inbound_substream()
                {
                    break inbound;
                }
            };

            upgrade::apply_inbound(inbound, GossipProtocol)
                .await
                .map_err(|_| Error::SubstreamProtocolUpgradeFailed(peer_id.short()))?
        }
    };

    spawn_substream_io_task(
        peer_id.clone(),
        substream,
        message_receiver,
        internal_event_sender_clone,
    );

    internal_event_sender
        .send(InternalEvent::ConnectionEstablished {
            peer_id,
            peer_info,
            origin,
            message_sender,
        })
        .map_err(|_| Error::InternalEventSendFailure("ConnectionEstablished"))?;

    Ok(())
}

fn spawn_substream_io_task(
    peer_id: PeerId,
    mut substream: GossipSubstream,
    message_receiver: DataReceiver,
    mut internal_event_sender: InternalEventSender,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // Allocate enough memory to store incoming messages from that peer. Messages have a size limit of 32kb.
        const MESSAGE_BUFFER_SIZE: usize = 32768;

        let mut fused_message_receiver = message_receiver.fuse();
        let mut message_buffer = vec![0u8; MESSAGE_BUFFER_SIZE];

        loop {
            select! {
                message = fused_message_receiver.next() => {
                    trace!("Outgoing message channel event.");
                    if let Some(message) = message {
                        if let Err(e) = send_message(&mut substream, &message).await {
                            error!("Failed to send message. Cause: {:?}", e);
                            continue;
                        }
                    } else {
                        trace!("Dropping connection");
                        break;
                    }

                }
                recv_result = recv_message(&mut substream, &mut message_buffer).fuse() => {
                    trace!("Incoming substream event.");
                    match recv_result {
                        Ok(num_read) => {
                            if let Err(e) = process_read(peer_id.clone(), num_read, &mut internal_event_sender, &message_buffer).await
                            {
                                error!("Failed to read message: Cause: {:?}", e);
                            }
                        }
                        Err(e) => {
                            debug!("{:?}", e);

                            if let Err(e) = internal_event_sender
                                .send(InternalEvent::ConnectionDropped {
                                    peer_id: peer_id.clone(),
                                })
                                .map_err(|_| Error::InternalEventSendFailure("ConnectionDropped"))
                            {
                                error!("Internal error. Cause: {:?}", e);
                            }


                            trace!("Remote dropped connection.");
                            break;
                        }
                    }
                }
            }
        }
        trace!("Shutting down connection handler task.");
    })
}

async fn send_message<S>(stream: &mut S, message: &[u8]) -> Result<(), Error>
where
    S: AsyncWrite + Unpin,
{
    stream.write_all(message).await.map_err(|_| Error::MessageSendError)?;
    stream.flush().await.map_err(|_| Error::MessageSendError)?;

    trace!("Wrote {} bytes to stream.", message.len());
    Ok(())
}

async fn recv_message<S>(stream: &mut S, message: &mut [u8]) -> Result<usize, Error>
where
    S: AsyncRead + Unpin,
{
    let num_read = stream.read(message).await.map_err(|_| Error::MessageRecvError)?;
    if num_read == 0 {
        // EOF
        debug!("Stream was closed remotely (EOF).");
        return Err(Error::StreamClosedByRemote);
    }

    trace!("Read {} bytes from stream.", num_read);
    Ok(num_read)
}

async fn process_read(
    peer_id: PeerId,
    num_read: usize,
    internal_event_sender: &mut InternalEventSender,
    buffer: &[u8],
) -> Result<(), Error> {
    let message = buffer[..num_read].to_vec();

    internal_event_sender
        .send(InternalEvent::MessageReceived { message, from: peer_id })
        .map_err(|_| Error::InternalEventSendFailure("MessageReceived"))?;

    Ok(())
}

/// Describes direction of an established connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    /// The connection is inbound (server).
    Inbound,
    /// The connection is outbound (client).
    Outbound,
}

impl Origin {
    /// Returns whether the connection is inbound.
    pub fn is_inbound(&self) -> bool {
        *self == Origin::Inbound
    }

    /// Returns whether the connection is outbound.
    pub fn is_outbound(&self) -> bool {
        *self == Origin::Outbound
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match *self {
            Origin::Outbound => "outbound",
            Origin::Inbound => "inbound",
        };
        write!(f, "{}", s)
    }
}
