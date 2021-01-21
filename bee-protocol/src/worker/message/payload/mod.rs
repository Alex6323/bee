// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

mod indexation;
mod milestone;
mod transaction;

pub(crate) use indexation::{IndexationPayloadWorker, IndexationPayloadWorkerEvent};
pub(crate) use milestone::{MilestonePayloadWorker, MilestonePayloadWorkerEvent};
pub(crate) use transaction::{TransactionPayloadWorker, TransactionPayloadWorkerEvent};

use crate::{storage::StorageBackend, worker::TangleWorker};

use bee_message::{payload::Payload, MessageId};
use bee_runtime::{node::Node, shutdown_stream::ShutdownStream, worker::Worker};
use bee_tangle::MsTangle;

use async_trait::async_trait;
use futures::stream::StreamExt;
use log::{info, warn};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use std::{any::TypeId, convert::Infallible};

#[derive(Debug)]
pub(crate) struct PayloadWorkerEvent(pub(crate) MessageId);

pub(crate) struct PayloadWorker {
    pub(crate) tx: mpsc::UnboundedSender<PayloadWorkerEvent>,
}

async fn process<B: StorageBackend>(
    tangle: &MsTangle<B>,
    message_id: MessageId,
    transaction_payload_worker: &mpsc::UnboundedSender<TransactionPayloadWorkerEvent>,
    milestone_payload_worker: &mpsc::UnboundedSender<MilestonePayloadWorkerEvent>,
    indexation_payload_worker: &mpsc::UnboundedSender<IndexationPayloadWorkerEvent>,
) {
    if let Some(message) = tangle.get(&message_id).await.map(|m| (*m).clone()) {
        match message.payload() {
            Some(Payload::Transaction(_)) => {
                if let Err(e) = transaction_payload_worker.send(TransactionPayloadWorkerEvent(message_id)) {
                    warn!(
                        "Sending message id {} to transaction payload worker failed: {:?}.",
                        message_id, e
                    );
                }
            }
            Some(Payload::Milestone(_)) => {
                if let Err(e) = milestone_payload_worker.send(MilestonePayloadWorkerEvent(message_id)) {
                    warn!(
                        "Sending message id {} to milestone payload worker failed: {:?}.",
                        message_id, e
                    );
                }
            }
            Some(Payload::Indexation(_)) => {
                if let Err(e) = indexation_payload_worker.send(IndexationPayloadWorkerEvent(message_id)) {
                    warn!(
                        "Sending message id {} to indexation payload worker failed: {:?}.",
                        message_id, e
                    );
                }
            }
            Some(_) => {
                // TODO
            }
            None => {
                // TODO
            }
        }
    }
}

#[async_trait]
impl<N> Worker<N> for PayloadWorker
where
    N: Node,
    N::Backend: StorageBackend,
{
    type Config = ();
    type Error = Infallible;

    fn dependencies() -> &'static [TypeId] {
        vec![
            TypeId::of::<TangleWorker>(),
            TypeId::of::<TransactionPayloadWorker>(),
            TypeId::of::<MilestonePayloadWorker>(),
            TypeId::of::<IndexationPayloadWorker>(),
        ]
        .leak()
    }

    async fn start(node: &mut N, _config: Self::Config) -> Result<Self, Self::Error> {
        let (tx, rx) = mpsc::unbounded_channel();
        let tangle = node.resource::<MsTangle<N::Backend>>();
        let transaction_payload_worker = node.worker::<TransactionPayloadWorker>().unwrap().tx.clone();
        let milestone_payload_worker = node.worker::<MilestonePayloadWorker>().unwrap().tx.clone();
        let indexation_payload_worker = node.worker::<IndexationPayloadWorker>().unwrap().tx.clone();

        node.spawn::<Self, _, _>(|shutdown| async move {
            info!("Running.");

            let mut receiver = ShutdownStream::new(shutdown, UnboundedReceiverStream::new(rx));

            while let Some(PayloadWorkerEvent(message_id)) = receiver.next().await {
                process(
                    &tangle,
                    message_id,
                    &transaction_payload_worker,
                    &milestone_payload_worker,
                    &indexation_payload_worker,
                )
                .await;
            }

            // let (_, mut receiver) = receiver.split();
            // let receiver = receiver.get_mut();
            // let mut count: usize = 0;
            //
            // while let Ok(PayloadWorkerEvent(message_id)) = receiver.try_recv() {
            //     process(
            //         &tangle,
            //         message_id,
            //         &transaction_payload_worker,
            //         &milestone_payload_worker,
            //         &indexation_payload_worker,
            //     )
            //     .await;
            //     count += 1;
            // }
            //
            // debug!("Drained {} message ids.", count);

            info!("Stopped.");
        });

        Ok(Self { tx })
    }
}
