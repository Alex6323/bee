// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::{
    config::TangleConfig,
    metadata::{IndexId, MessageMetadata},
    solid_entry_point::SolidEntryPoint,
    storage::StorageBackend,
    tangle::{Hooks, Tangle, DEFAULT_CACHE_LEN},
    urts::UrtsTipPool,
    MessageRef,
};

use bee_message::{
    milestone::{Milestone, MilestoneIndex},
    Message, MessageId,
};
use bee_runtime::resource::ResourceHandle;

use hashbrown::HashMap;
use log::{info, trace};
use ref_cast::RefCast;
use tokio::sync::Mutex;

use std::{
    ops::Deref,
    sync::atomic::{AtomicU32, Ordering},
};

const SYNCED_THRESHOLD: u32 = 2;
const CONFIRMED_THRESHOLD: u32 = 2;

/// Tangle hooks that interoperate with Bee's storage layer.
pub struct StorageHooks<B> {
    #[allow(dead_code)]
    storage: ResourceHandle<B>,
}

impl<B: StorageBackend> Hooks<MessageMetadata> for StorageHooks<B> {
    type Error = B::Error;

    fn get(&self, id: &MessageId) -> Result<Option<(Message, MessageMetadata)>, Self::Error> {
        trace!("Attempted to fetch message {:?}", id);
        let msg = self.storage.fetch(id)?;
        let meta = self.storage.fetch(id)?;
        Ok(msg.zip(meta))
    }

    fn insert(&self, id: MessageId, tx: Message, metadata: MessageMetadata) -> Result<(), Self::Error> {
        trace!("Attempted to insert message {:?}", id);
        self.storage.insert(&id, &tx)?;
        self.storage.insert(&id, &metadata)?;
        Ok(())
    }

    fn fetch_approvers(&self, id: &MessageId) -> Result<Option<Vec<MessageId>>, Self::Error> {
        trace!("Attempted to fetch approvers for message {:?}", id);
        self.storage.fetch(id)
    }

    fn insert_approver(&self, id: MessageId, approver: MessageId) -> Result<(), Self::Error> {
        trace!("Attempted to insert approver for message {:?}", id);
        self.storage.insert(&(id, approver), &())
    }

    fn update_approvers(&self, id: MessageId, approvers: &[MessageId]) -> Result<(), Self::Error> {
        trace!("Attempted to update approvers for message {:?}", id);
        for approver in approvers {
            self.storage.insert(&(id, *approver), &())?;
        }
        Ok(())
    }
}

impl<B: StorageBackend> StorageHooks<B> {
    fn get_milestone(&self, idx: &MilestoneIndex) -> Result<Option<Milestone>, B::Error> {
        trace!("Attempted to fetch milestone {:?}", idx);
        self.storage.fetch(idx)
    }

    fn insert_milestone(&self, idx: MilestoneIndex, milestone: &Milestone) -> Result<(), B::Error> {
        trace!("Attempted to insert milestone {:?}", idx);
        self.storage.insert(&idx, milestone)?;
        Ok(())
    }
}

/// A Tangle wrapper designed to encapsulate milestone state.
pub struct MsTangle<B> {
    config: TangleConfig,
    inner: Tangle<MessageMetadata, StorageHooks<B>>,
    milestones: Mutex<HashMap<MilestoneIndex, Milestone>>,
    solid_entry_points: Mutex<HashMap<SolidEntryPoint, MilestoneIndex>>,
    latest_milestone_index: AtomicU32,
    solid_milestone_index: AtomicU32,
    confirmed_milestone_index: AtomicU32,
    snapshot_index: AtomicU32,
    pruning_index: AtomicU32,
    entry_point_index: AtomicU32,
    tip_pool: Mutex<UrtsTipPool>,
}

impl<B> Deref for MsTangle<B> {
    type Target = Tangle<MessageMetadata, StorageHooks<B>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<B: StorageBackend> MsTangle<B> {
    /// Create a new `MsTangle` instance with the given configuration and storage handle.
    pub fn new(config: TangleConfig, storage: ResourceHandle<B>) -> Self {
        Self {
            inner: Tangle::new(StorageHooks { storage }),
            milestones: Default::default(),
            solid_entry_points: Default::default(),
            latest_milestone_index: Default::default(),
            solid_milestone_index: Default::default(),
            confirmed_milestone_index: Default::default(),
            snapshot_index: Default::default(),
            pruning_index: Default::default(),
            entry_point_index: Default::default(),
            tip_pool: Mutex::new(UrtsTipPool::new(&config)),
            config,
        }
    }

    /// Shut down the tangle, terminating any and all worker tasks.
    pub async fn shutdown(self) {
        // TODO: Write back changes by calling self.inner.shutdown().await
    }

    /// Get the configuration of this tangle.
    pub fn config(&self) -> &TangleConfig {
        &self.config
    }

    /// Insert a message into the tangle.
    pub async fn insert(&self, message: Message, hash: MessageId, metadata: MessageMetadata) -> Option<MessageRef> {
        self.inner.insert(hash, message, metadata).await
    }

    /// Add a milestone to the tangle.
    pub async fn add_milestone(&self, idx: MilestoneIndex, milestone: Milestone) {
        // TODO: only insert if vacant
        self.inner
            .update_metadata(&milestone.message_id(), |metadata| {
                metadata.flags_mut().set_milestone(true);
                metadata.set_milestone_index(idx);
                metadata.set_omrsi(IndexId::new(idx, *milestone.message_id()));
                metadata.set_ymrsi(IndexId::new(idx, *milestone.message_id()));
            })
            .await;
        self.inner
            .hooks()
            .insert_milestone(idx, &milestone)
            .unwrap_or_else(|e| info!("Failed to insert message {:?}", e));
        self.milestones.lock().await.insert(idx, milestone);
    }

    /// Remove a milestone from the tangle.
    pub async fn remove_milestone(&self, index: MilestoneIndex) {
        self.milestones.lock().await.remove(&index);
    }

    async fn pull_milestone(&self, idx: MilestoneIndex) -> Option<MessageId> {
        if let Some(milestone) = self.inner.hooks().get_milestone(&idx).unwrap_or_else(|e| {
            info!("Failed to insert message {:?}", e);
            None
        }) {
            let message_id = *self
                .milestones
                .lock()
                .await
                .entry(idx)
                .or_insert(milestone)
                .message_id();

            Some(message_id)
        } else {
            None
        }
    }

    /// Get the milestone from the tangle that corresponds to the given milestone index.
    pub async fn get_milestone(&self, index: MilestoneIndex) -> Option<Milestone> {
        self.milestones.lock().await.get(&index).cloned()
    }

    /// Get the message associated with the given milestone index from the tangle.
    pub async fn get_milestone_message(&self, index: MilestoneIndex) -> Option<MessageRef> {
        // TODO: use combinator instead of match
        match self.get_milestone_message_id(index).await {
            None => None,
            Some(ref hash) => self.get(hash).await,
        }
    }

    /// Get the message ID associated with the given milestone index from the tangle.
    pub async fn get_milestone_message_id(&self, index: MilestoneIndex) -> Option<MessageId> {
        let message_id = self.milestones.lock().await.get(&index).map(|m| *m.message_id());

        // TODO: use combinator instead of match
        match message_id {
            Some(m) => Some(m),
            None => Some(self.pull_milestone(index).await?),
        }
    }

    /// Return whether the tangle contains the given milestone index.
    pub async fn contains_milestone(&self, idx: MilestoneIndex) -> bool {
        // Not using `||` as its first operand would keep the lock alive causing a deadlock with its second operand.
        if self.milestones.lock().await.contains_key(&idx) {
            return true;
        }
        self.pull_milestone(idx).await.is_some()
    }

    /// Get the index of the latest milestone.
    pub fn get_latest_milestone_index(&self) -> MilestoneIndex {
        self.latest_milestone_index.load(Ordering::Relaxed).into()
    }

    /// Update the index of the lastest milestone.
    pub fn update_latest_milestone_index(&self, new_index: MilestoneIndex) {
        // TODO: `fetch_max`? Swap and ensure the old is smaller?
        self.latest_milestone_index.store(*new_index, Ordering::Relaxed);
    }

    /// Get the latest solid milestone index.
    pub fn get_solid_milestone_index(&self) -> MilestoneIndex {
        self.solid_milestone_index.load(Ordering::Relaxed).into()
    }

    /// Update the latest solid milestone index.
    pub fn update_solid_milestone_index(&self, new_index: MilestoneIndex) {
        self.solid_milestone_index.store(*new_index, Ordering::Relaxed);

        // TODO: Formalise this a little better
        let new_len = ((1000.0 + self.get_sync_threshold() as f32 * 500.0) as usize)
            .min(DEFAULT_CACHE_LEN)
            .max(8192);
        self.inner.resize(new_len);
    }

    /// Get the latest confirmed milestone index.
    pub fn get_confirmed_milestone_index(&self) -> MilestoneIndex {
        self.confirmed_milestone_index.load(Ordering::Relaxed).into()
    }

    /// Update the latest confirmed milestone index.
    pub fn update_confirmed_milestone_index(&self, new_index: MilestoneIndex) {
        self.confirmed_milestone_index.store(*new_index, Ordering::Relaxed);
    }

    /// Get the snapshot index.
    pub fn get_snapshot_index(&self) -> MilestoneIndex {
        self.snapshot_index.load(Ordering::Relaxed).into()
    }

    /// Update the snapshot index.
    pub fn update_snapshot_index(&self, new_index: MilestoneIndex) {
        self.snapshot_index.store(*new_index, Ordering::Relaxed);
    }

    /// Get the pruning index.
    pub fn get_pruning_index(&self) -> MilestoneIndex {
        self.pruning_index.load(Ordering::Relaxed).into()
    }

    /// Update the pruning index.
    pub fn update_pruning_index(&self, new_index: MilestoneIndex) {
        self.pruning_index.store(*new_index, Ordering::Relaxed);
    }

    /// Get the entry point index.
    pub fn get_entry_point_index(&self) -> MilestoneIndex {
        self.entry_point_index.load(Ordering::Relaxed).into()
    }

    /// Update the entry point index.
    pub fn update_entry_point_index(&self, new_index: MilestoneIndex) {
        self.entry_point_index.store(*new_index, Ordering::Relaxed);
    }

    /// Return whether the tangle is within the default sync threshold.
    pub fn is_synced(&self) -> bool {
        // TODO reduce to one atomic value ?
        self.is_synced_threshold(SYNCED_THRESHOLD)
    }

    /// Get the number of milestones until the tangle is synced.
    pub fn get_sync_threshold(&self) -> u32 {
        // TODO reduce to one atomic value ?
        self.get_latest_milestone_index()
            .saturating_sub(*self.get_solid_milestone_index())
    }

    /// Return whether the tangle is within the given sync threshold.
    pub fn is_synced_threshold(&self, threshold: u32) -> bool {
        // TODO reduce to one atomic value ?
        *self.get_solid_milestone_index() >= self.get_latest_milestone_index().saturating_sub(threshold)
    }

    /// Return whether the tangle is fully confirmed.
    pub fn is_confirmed(&self) -> bool {
        // TODO reduce to one atomic value ?
        self.is_confirmed_threshold(CONFIRMED_THRESHOLD)
    }

    /// Return whether the tangle is within the given confirmation threshold.
    pub fn is_confirmed_threshold(&self, threshold: u32) -> bool {
        // TODO reduce to one atomic value ?
        *self.get_confirmed_milestone_index() >= self.get_latest_milestone_index().saturating_sub(threshold)
    }

    /// Get the milestone index associated with the given solid entry point.
    pub async fn get_solid_entry_point_index(&self, sep: &SolidEntryPoint) -> Option<MilestoneIndex> {
        self.solid_entry_points.lock().await.get(sep).copied()
    }

    /// Add the given solid entry point to the given milestone index.
    pub async fn add_solid_entry_point(&self, sep: SolidEntryPoint, index: MilestoneIndex) {
        self.solid_entry_points.lock().await.insert(sep, index);
    }

    /// Returns a copy of all solid entry points.
    pub async fn get_solid_entry_points(&self) -> HashMap<SolidEntryPoint, MilestoneIndex> {
        self.solid_entry_points.lock().await.clone()
    }

    /// Removes the given solid entry point from the set of solid entry points.
    pub async fn remove_solid_entry_point(&self, sep: &SolidEntryPoint) {
        self.solid_entry_points.lock().await.remove(sep);
    }

    /// Clear all solid entry points.
    pub async fn clear_solid_entry_points(&self) {
        self.solid_entry_points.lock().await.clear();
    }

    /// Replaces all solid entry points.
    pub async fn replace_solid_entry_points(
        &self,
        new_seps: impl IntoIterator<Item = (SolidEntryPoint, MilestoneIndex)>,
    ) {
        let mut seps = self.solid_entry_points.lock().await;
        seps.clear();
        seps.extend(new_seps);
    }

    /// Returns whether the message associated with given solid entry point is a solid entry point.
    pub async fn is_solid_entry_point(&self, id: &MessageId) -> bool {
        self.solid_entry_points
            .lock()
            .await
            .contains_key(SolidEntryPoint::ref_cast(id))
    }

    /// Returns whether the message associated with the given message ID is solid.
    pub async fn is_solid_message(&self, id: &MessageId) -> bool {
        if self.is_solid_entry_point(id).await {
            true
        } else {
            self.inner
                .get_metadata(id)
                .await
                .map(|metadata| metadata.flags().is_solid())
                .unwrap_or(false)
        }
    }

    /// Get the oldest milestone root snapshot index.
    pub async fn omrsi(&self, id: &MessageId) -> Option<IndexId> {
        match self.solid_entry_points.lock().await.get(SolidEntryPoint::ref_cast(id)) {
            Some(sep) => Some(IndexId::new(*sep, *id)),
            None => match self.get_metadata(id).await {
                Some(metadata) => metadata.omrsi(),
                None => None,
            },
        }
    }

    /// Get the youngest milestone root snapshot index.
    pub async fn ymrsi(&self, id: &MessageId) -> Option<IndexId> {
        match self.solid_entry_points.lock().await.get(SolidEntryPoint::ref_cast(id)) {
            Some(sep) => Some(IndexId::new(*sep, *id)),
            None => match self.get_metadata(id).await {
                Some(metadata) => metadata.ymrsi(),
                None => None,
            },
        }
    }

    /// Insert the given message ID and parents as a tip.
    pub async fn insert_tip(&self, message_id: MessageId, parents: Vec<MessageId>) {
        self.tip_pool.lock().await.insert(&self, message_id, parents).await;
    }

    /// Update tip scores.
    pub async fn update_tip_scores(&self) {
        self.tip_pool.lock().await.update_scores(&self).await;
    }

    /// Return messages that require approving.
    pub async fn get_messages_to_approve(&self) -> Option<Vec<MessageId>> {
        self.tip_pool.lock().await.choose_non_lazy_tips()
    }

    /// Reduce tips.
    pub async fn reduce_tips(&self) {
        self.tip_pool.lock().await.reduce_tips();
    }

    /// Return the number of non-lazy tips.
    pub async fn non_lazy_tips_num(&self) -> usize {
        self.tip_pool.lock().await.non_lazy_tips().len()
    }
}
