// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

pub mod connection;
pub mod handler;
pub mod io;
pub mod protocol;
pub mod upgrade;

pub const PROTOCOL_INFO_IOTA_GOSSIP: &str = "iota-gossip";
pub const PROTOCOL_INFO_VERSION: &str = "1.0.0";
