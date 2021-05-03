// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0
use super::info::IotaGossipInfo;

use futures::{future, AsyncRead, AsyncWrite};
use libp2p::{core::UpgradeInfo, InboundUpgrade, OutboundUpgrade};
use log::trace;

use std::{io, iter};

#[derive(Debug, Clone)]
pub struct IotaGossipProtocolUpgrade {
    info: IotaGossipInfo,
}

impl IotaGossipProtocolUpgrade {
    pub fn new(info: IotaGossipInfo) -> Self {
        Self { info }
    }
}

impl UpgradeInfo for IotaGossipProtocolUpgrade {
    type Info = IotaGossipInfo;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        trace!("gossip upgrade: protocol info query: {}", self.info);

        iter::once(self.info.clone())
    }
}

impl<S> InboundUpgrade<S> for IotaGossipProtocolUpgrade
where
    S: AsyncWrite + Unpin,
{
    type Output = S;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, io::Error>>;

    fn upgrade_inbound(self, stream: S, info: Self::Info) -> Self::Future {
        trace!("gossip upgrade: inbound: {}", info);

        future::ok(stream)
    }
}

impl<S> OutboundUpgrade<S> for IotaGossipProtocolUpgrade
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = S;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, io::Error>>;

    fn upgrade_outbound(self, stream: S, info: Self::Info) -> Self::Future {
        trace!("gossip upgrade: outbound: {}", info);

        future::ok(stream)
    }
}
