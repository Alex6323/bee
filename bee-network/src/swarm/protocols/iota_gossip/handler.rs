// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use super::{event::IotaGossipHandlerEvent, info::IotaGossipInfo, upgrade::IotaGossipProtocolUpgrade};

use crate::network::meta::Origin;

use libp2p::{
    core::upgrade::OutboundUpgrade,
    swarm::{
        protocols_handler::{
            KeepAlive, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, SubstreamProtocol,
        },
        NegotiatedSubstream,
    },
    Multiaddr, PeerId,
};

use log::*;

use std::{
    collections::VecDeque,
    io,
    task::{Context, Poll},
    time::{Duration, Instant},
};

const KEEP_ALIVE_UNTIL: Duration = Duration::from_secs(30);

pub struct GossipProtocolHandler {
    ///
    info: IotaGossipInfo,

    /// Keep the connection alive for some time.
    keep_alive: KeepAlive,

    ///
    inbound_index: usize,

    ///
    outbound_index: usize,

    ///
    in_events: VecDeque<IotaGossipHandlerInEvent>,

    ///
    errors: VecDeque<(usize, ProtocolsHandlerUpgrErr<io::Error>)>,

    /// Eventually determined [`PeerId`].
    peer_id: Option<PeerId>,

    /// Eventually determined [`Multiaddr`].
    peer_addr: Option<Multiaddr>,

    /// Eventually determined [`Origin`].
    conn_origin: Option<Origin>,

    /// Eventually determined [`NegotiatedSubstream`].
    inbound: Option<NegotiatedSubstream>,

    /// Eventually determined [`NegotiatedSubstream`].
    outbound: Option<NegotiatedSubstream>,
}

#[derive(Debug)]
pub struct IotaGossipHandlerInEvent {
    pub peer_id: PeerId,
    pub peer_addr: Multiaddr,
    pub conn_origin: Origin,
}

impl GossipProtocolHandler {
    pub fn new(inbound_index: usize, outbound_index: usize, info: IotaGossipInfo) -> Self {
        Self {
            info,
            keep_alive: KeepAlive::Until(Instant::now() + KEEP_ALIVE_UNTIL),
            inbound_index,
            outbound_index,
            in_events: VecDeque::with_capacity(16),
            errors: VecDeque::with_capacity(16),
            peer_id: None,
            peer_addr: None,
            conn_origin: None,
            inbound: None,
            outbound: None,
        }
    }
}

impl ProtocolsHandler for GossipProtocolHandler {
    type InEvent = IotaGossipHandlerInEvent;
    type OutEvent = IotaGossipHandlerEvent;
    type Error = io::Error;
    type InboundProtocol = IotaGossipProtocolUpgrade;
    type OutboundProtocol = IotaGossipProtocolUpgrade;
    type InboundOpenInfo = usize;
    type OutboundOpenInfo = usize;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        debug!("IOTA gossip handler: get listen protocol: {}", self.inbound_index);

        SubstreamProtocol::new(IotaGossipProtocolUpgrade::new(self.info.clone()), self.inbound_index)
    }

    fn inject_event(&mut self, in_event: IotaGossipHandlerInEvent) {
        debug!("IOTA gossip handler: received in-event: {:?}", in_event);

        self.in_events.push_back(in_event);
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        new_inbound: NegotiatedSubstream,
        inbound_index: Self::InboundOpenInfo,
    ) {
        debug!("IOTA gossip handler: fully negotiated inbound: {}", inbound_index);

        if self.inbound.is_none() {
            self.inbound.replace(new_inbound);
        } else {
            warn!("Inbound substream already exists.");
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        new_outbound: NegotiatedSubstream,
        outbound_index: Self::OutboundOpenInfo,
    ) {
        debug!("IOTA gossip handler: fully negotiated outbound: {}", outbound_index);

        if self.outbound.is_none() {
            self.outbound.replace(new_outbound);
        } else {
            warn!("Outbound substream already exists.");
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        outbound_index: Self::OutboundOpenInfo,
        e: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Error>,
    ) {
        debug!("IOTA gossip handler: outbound upgrade error: {:?}", e);

        self.errors.push_back((outbound_index, e));
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        debug!("IOTA gossip handler: return KeepAlive variant.");

        self.keep_alive
    }

    // #[allow(clippy::type_complexity)]
    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent, Self::Error>> {
        // Process swarm updates.
        while let Some(in_event) = self.in_events.pop_front() {
            let IotaGossipHandlerInEvent {
                peer_id,
                peer_addr,
                conn_origin,
            } = in_event;

            // Update state of this handler with new information.
            self.peer_id = Some(peer_id);
            self.peer_addr = Some(peer_addr);
            self.conn_origin = Some(conn_origin);

            // By convention only send the update request for outbound connections
            if conn_origin == Origin::Outbound {
                let request_sent_event = ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(
                        IotaGossipProtocolUpgrade::new(self.info.clone()),
                        self.outbound_index,
                    ),
                };

                debug!("IOTA gossip handler: protocol upgrade request sent.");

                return Poll::Ready(request_sent_event);
            } else {
                debug!("IOTA gossip handler: waiting for protocol upgrade request.");

                let waiting_for_request =
                    ProtocolsHandlerEvent::Custom(IotaGossipHandlerEvent::AwaitingUpgradeRequest { from: peer_id });

                return Poll::Ready(waiting_for_request);
            }
        }

        if let Some(inbound) = self.inbound.take() {
            debug!("IOTA gossip handler: negotiated new inbound stream.");

            return Poll::Ready(ProtocolsHandlerEvent::Custom(
                IotaGossipHandlerEvent::UpgradeCompleted {
                    peer_id: *self.peer_id.as_ref().unwrap(),
                    peer_addr: self.peer_addr.as_ref().unwrap().clone(),
                    conn_origin: *self.conn_origin.as_ref().unwrap(),
                    substream: inbound,
                },
            ));
        } else if let Some(outbound) = self.outbound.take() {
            debug!("IOTA gossip handler: negotiated new outbound stream.");

            return Poll::Ready(ProtocolsHandlerEvent::Custom(
                IotaGossipHandlerEvent::UpgradeCompleted {
                    peer_id: *self.peer_id.as_ref().unwrap(),
                    peer_addr: self.peer_addr.as_ref().unwrap().clone(),
                    conn_origin: *self.conn_origin.as_ref().unwrap(),
                    substream: outbound,
                },
            ));
        }

        Poll::Pending
    }
}
