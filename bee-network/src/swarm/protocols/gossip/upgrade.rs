// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::init::global::network_id;

use futures::{future, AsyncRead, AsyncWrite};
use libp2p::{core::UpgradeInfo, InboundUpgrade, OutboundUpgrade};
use log::trace;

use std::iter;

const PROTOCOL_INFO_IOTA_GOSSIP: &str = "iota-gossip";
const PROTOCOL_INFO_VERSION: &str = "1.0.0";

/// Configuration for an upgrade to the `IotaGossip` protocol.
#[derive(Debug, Clone, Default)]
pub struct GossipProtocolUpgrade;

impl UpgradeInfo for GossipProtocolUpgrade {
    type Info = Vec<u8>;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(
            format!(
                "/{}/{}/{}",
                PROTOCOL_INFO_IOTA_GOSSIP,
                network_id(),
                PROTOCOL_INFO_VERSION
            )
            .into_bytes(),
        )
    }
}

impl<C> InboundUpgrade<C> for GossipProtocolUpgrade
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    type Output = C;
    type Error = ();
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, stream: C, _: Self::Info) -> Self::Future {
        trace!("Upgrading inbound connection to gossip protocol.");

        // Just return the stream.
        future::ok(stream)
    }
}

impl<C> OutboundUpgrade<C> for GossipProtocolUpgrade
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = C;
    type Error = ();
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, stream: C, _: Self::Info) -> Self::Future {
        trace!("Upgrading outbound connection to gossip protocol.");

        // Just return the stream.
        future::ok(stream)
    }
}
