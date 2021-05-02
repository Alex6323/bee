// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use super::{
    event::{IotaGossipEvent, IotaGossipHandlerEvent},
    handler::{GossipProtocolHandler, IotaGossipHandlerInEvent},
    info::IotaGossipInfo,
};

use crate::{alias, init::global::network_id, network::meta::Origin};

use libp2p::{
    core::{connection::ConnectionId, ConnectedPoint},
    swarm::{NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters},
    Multiaddr, PeerId,
};
use log::trace;

use std::{
    collections::VecDeque,
    task::{Context, Poll},
};

const IOTA_GOSSIP_NAME: &str = "iota-gossip";
const IOTA_GOSSIP_VERSION: &str = "1.0.0";

/// Substream upgrade protocol for `/iota-gossip/1.0.0`.
#[derive(Debug)]
pub struct IotaGossipProtocol {
    info: IotaGossipInfo,
    num_handlers: usize,
    num_inbounds: usize,
    num_outbounds: usize,

    /// Events produced by the underlying swarm.
    swarm_events: VecDeque<SwarmEvent>,

    /// Events produced by the protocol handler.
    handler_events: VecDeque<HandlerEvent>,
}

#[derive(Debug)]
struct SwarmEvent {
    peer_id: PeerId,
    peer_addr: Multiaddr,
    conn_id: ConnectionId,
    origin: Origin,
}

#[derive(Debug)]
struct HandlerEvent {
    peer_id: PeerId,
    conn_id: ConnectionId,
    event: IotaGossipHandlerEvent,
}

impl IotaGossipProtocol {
    pub fn new() -> Self {
        Self {
            info: IotaGossipInfo::new(IOTA_GOSSIP_NAME.into(), network_id(), IOTA_GOSSIP_VERSION.into()),
            num_handlers: 0,
            num_inbounds: 0,
            num_outbounds: 0,
            swarm_events: VecDeque::with_capacity(16),
            handler_events: VecDeque::with_capacity(16),
        }
    }
}

impl NetworkBehaviour for IotaGossipProtocol {
    type ProtocolsHandler = GossipProtocolHandler;
    type OutEvent = IotaGossipEvent;

    // Order of events:
    // (1) new_handler
    // (2) inject_connection_established
    // (3) inject_connected

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        trace!("IOTA gossip protocol: new handler.");

        self.num_handlers += 1;

        GossipProtocolHandler::new(self.num_inbounds, self.num_outbounds, self.info.clone())
    }

    fn inject_connection_established(&mut self, peer_id: &PeerId, conn_id: &ConnectionId, endpoint: &ConnectedPoint) {
        let (peer_addr, origin) = match endpoint {
            ConnectedPoint::Dialer { address } => (address.clone(), Origin::Outbound),
            ConnectedPoint::Listener { send_back_addr, .. } => (send_back_addr.clone(), Origin::Inbound),
        };

        match origin {
            Origin::Inbound => self.num_inbounds += 1,
            Origin::Outbound => self.num_outbounds += 1,
        }

        let event = SwarmEvent {
            peer_id: *peer_id,
            peer_addr,
            conn_id: *conn_id,
            origin,
        };

        trace!("IOTA gossip protocol: swarm event: {:?}", event);

        self.swarm_events.push_back(event);
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        trace!("IOTA gossip protocol: {} connected.", alias!(peer_id));
    }

    fn inject_event(&mut self, peer_id: PeerId, conn_id: ConnectionId, event: IotaGossipHandlerEvent) {
        let event = HandlerEvent {
            peer_id,
            conn_id,
            event,
        };

        trace!("IOTA gossip protocol: handler event: {:?}", event);

        self.handler_events.push_back(event);
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<IotaGossipHandlerInEvent, Self::OutEvent>> {
        // Update the gossip handler with new information.
        while let Some(event) = self.swarm_events.pop_front() {
            let SwarmEvent {
                peer_id,
                peer_addr,
                conn_id,
                origin,
            } = event;

            let notify_handler = NetworkBehaviourAction::NotifyHandler {
                peer_id,
                handler: NotifyHandler::One(conn_id),
                event: IotaGossipHandlerInEvent {
                    peer_id,
                    peer_addr,
                    conn_origin: origin,
                },
            };

            return Poll::Ready(notify_handler);
        }

        // Process handler events.
        while let Some(event) = self.handler_events.pop_front() {
            let HandlerEvent {
                peer_id: _,
                conn_id: _,
                event,
            } = event;

            match event {
                IotaGossipHandlerEvent::AwaitingUpgradeRequest { from: _ } => {
                    // TODO
                }

                IotaGossipHandlerEvent::ReceivedUpgradeRequest { from: _ } => {
                    // TODO
                }

                IotaGossipHandlerEvent::SentUpgradeRequest { to } => {
                    let notify_swarm =
                        NetworkBehaviourAction::GenerateEvent(IotaGossipEvent::SentUpgradeRequest { to });

                    return Poll::Ready(notify_swarm);
                }

                IotaGossipHandlerEvent::UpgradeCompleted {
                    peer_id,
                    peer_addr,
                    conn_origin,
                    substream,
                } => {
                    let notify_swarm = NetworkBehaviourAction::GenerateEvent(IotaGossipEvent::UpgradeCompleted {
                        peer_id,
                        peer_addr,
                        conn_origin,
                        substream,
                    });

                    return Poll::Ready(notify_swarm);
                }

                IotaGossipHandlerEvent::UpgradeError { peer_id, error } => {
                    let notify_swarm =
                        NetworkBehaviourAction::GenerateEvent(IotaGossipEvent::UpgradeError { peer_id, error });

                    return Poll::Ready(notify_swarm);
                }
            }
        }

        Poll::Pending
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        trace!("IOTA gossip protocol: return addresses of peer {}.", alias!(peer_id));
        Vec::new()
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        trace!("gossip behavior: {} disconnected.", alias!(peer_id));
    }
}
