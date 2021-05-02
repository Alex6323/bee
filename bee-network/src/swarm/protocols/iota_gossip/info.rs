// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

// use std::convert::TryFrom;

use libp2p_core::ProtocolName;

#[derive(Debug, Clone)]
pub struct IotaGossipInfo {
    name: String,
    network_id: u64,
    version: String,
    buffered: Vec<u8>,
}

impl IotaGossipInfo {
    pub fn new(name: String, network_id: u64, version: String) -> Self {
        let buffered = format!("/{}/{}/{}", name, network_id, version).into_bytes();

        Self {
            name: name.to_string(),
            network_id,
            version: version.to_string(),
            buffered,
        }
    }
}

impl ProtocolName for IotaGossipInfo {
    fn protocol_name(&self) -> &[u8] {
        &self.buffered
    }
}
