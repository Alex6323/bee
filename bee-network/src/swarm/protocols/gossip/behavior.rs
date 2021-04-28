// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use super::{
    event::{GossipEvent, GossipEventBuilder},
    handler::GossipHandler,
};

use crate::network::meta::{ConnectionInfo, Origin};

use libp2p::{
    core::{connection::ConnectionId, ConnectedPoint},
    swarm::{NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction, ProtocolsHandler},
    PeerId,
};
use log::trace;

use std::{
    collections::{HashMap, VecDeque},
    sync::atomic::{AtomicBool, Ordering},
    task::Poll,
};

// FIXME: Due to limitations of the libp2p's `ProtocolHandler` necessary at the moment. Could cause a failed gossip
// protocol negotiation with a peer due to a race condition. But due to the fact that peers can retry to connect to
// eachother, the probability of not being able to peer with eachother becomes negligable. In fact, this code has
// already shown that it is working as intended for all practical purposes. However, a perfect implementation would not
// have even allow the theoretical chance of such a failure, which is why this needs to be fixed eventually. The purpose
// of this atomic bool is to inject more state into the `Gossip` type which is instantiated by `libp2p`. This is
// required because depending on that state the peers are supposed to act differently. By IOTAs convention the dialer
// has to issue the protocol request, while the dialed one has to respond.
pub static GOSSIP_ORIGIN: AtomicBool = AtomicBool::new(false);

#[derive(Eq, PartialEq, Hash, Debug, Clone, Copy)]
struct Id(PeerId, ConnectionId);

#[derive(Default)]
pub struct Gossip {
    builders: HashMap<PeerId, GossipEventBuilder>,
    // events produced by gossip handlers
    events: VecDeque<GossipEvent>,
}

impl Gossip {
    pub fn new() -> Self {
        Self::default()
    }
}

impl NetworkBehaviour for Gossip {
    type ProtocolsHandler = GossipHandler;
    type OutEvent = GossipEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        // FIXME:
        let origin = if GOSSIP_ORIGIN.swap(false, Ordering::SeqCst) {
            Origin::Outbound
        } else {
            Origin::Inbound
        };

        trace!("GOSSIP: new_handler: {}", origin);
        GossipHandler::new(origin)
    }

    fn addresses_of_peer(&mut self, peer_id: &libp2p::PeerId) -> Vec<libp2p::Multiaddr> {
        trace!("Addresses of peer: {}", peer_id);
        Vec::new()
    }

    fn inject_connection_established(&mut self, peer_id: &PeerId, conn_id: &ConnectionId, endpoint: &ConnectedPoint) {
        // TODO: Perform the connection checks (not banned, not a duplicate etc)

        let (address, origin) = match endpoint {
            ConnectedPoint::Dialer { address } => (address.clone(), Origin::Outbound),
            ConnectedPoint::Listener { send_back_addr, .. } => (send_back_addr.clone(), Origin::Inbound),
        };

        let builder = GossipEventBuilder::default()
            .with_peer_id(*peer_id)
            .with_peer_addr(address)
            .with_conn_info(ConnectionInfo { id: *conn_id, origin });

        self.builders.insert(*peer_id, builder);
    }

    fn inject_connected(&mut self, _peer_id: &libp2p::PeerId) {}

    fn inject_event(&mut self, peer_id: PeerId, _: ConnectionId, conn: NegotiatedSubstream) {
        if let Some(builder) = self.builders.remove(&peer_id) {
            self.events.push_back(builder.with_conn(conn).finish());
        }
    }

    fn inject_disconnected(&mut self, _: &libp2p::PeerId) {}

    fn poll(
        &mut self,
        _: &mut std::task::Context<'_>,
        _: &mut impl libp2p::swarm::PollParameters,
    ) -> Poll<NetworkBehaviourAction<<Self::ProtocolsHandler as ProtocolsHandler>::InEvent, Self::OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            Poll::Ready(NetworkBehaviourAction::GenerateEvent(event))
        } else {
            Poll::Pending
        }
    }
}