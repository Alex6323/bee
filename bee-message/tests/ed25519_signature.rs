// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use bee_message::prelude::*;

#[test]
fn kind() {
    assert_eq!(Ed25519Signature::KIND, 0);
}
