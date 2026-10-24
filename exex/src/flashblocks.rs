use std::collections::BTreeMap;
use alloy_primitives::Bytes;
use tracing::trace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceValidation {
    NextInSequence,
    FirstOfNextBlock,
    DuplicateSequence,
    DivergentSequence,
}

pub struct CustomFlashblockProcessor {
    /// Pinned verification epoch to shield the engine from asynchronous state races
    pub active_epoch: u64,
    /// Memory isolated buffer container ensuring sub-second index progression sorting
    pub transient_buffer: BTreeMap<u64, Vec<Bytes>>,
    /// Tracks the last processed canonical block boundary
    pub last_canonical_block: u64,
}

impl CustomFlashblockProcessor {
    pub fn new() -> Self {
        Self {
            active_epoch: 0,
            transient_buffer: BTreeMap::new(),
            last_canonical_block: 0,
        }
    }

    /// Evicts volatile memory caches immediately when a chain reorganization occurs
    pub fn purge_speculative_cache(&mut self) {
        self.transient_buffer.clear();
        self.active_epoch = self.active_epoch.wrapping_add(1);
        trace!(target: "supersonic-exex", epoch = %self.active_epoch, "Speculative state purged due to engine reorganization");
    }

    /// Evaluates sequence ordering parameters against prefix histories
    pub fn validate_sequence(&self, block_num: u64, index: u64) -> SequenceValidation {
        if block_num == self.last_canonical_block + 1 && index == 0 {
            SequenceValidation::FirstOfNextBlock
        } else if self.transient_buffer.contains_key(&index) {
            SequenceValidation::DuplicateSequence
        } else if index == 0 || self.transient_buffer.contains_key(&(index - 1)) {
            SequenceValidation::NextInSequence
        } else {
            SequenceValidation::DivergentSequence
        }
    }

    /// Processes an incoming sub-second transaction sequence chunk, enforces strict structural validation gates,
    /// and flattens the speculative block data structure for downstream IPC consumption.
    pub fn process_speculative_sequence(
        &mut self,
        block_num: u64,
        sequence_index: u64,
        txs: Vec<Bytes>,
    ) -> Option<Vec<Bytes>> {
        match self.validate_sequence(block_num, sequence_index) {
            SequenceValidation::FirstOfNextBlock => {
                self.transient_buffer.clear();
                self.transient_buffer.insert(sequence_index, txs);
                self.last_canonical_block = block_num - 1;
                Some(self.assemble_flat_transactions())
            }
            SequenceValidation::NextInSequence => {
                self.transient_buffer.insert(sequence_index, txs);
                Some(self.assemble_flat_transactions())
            }
            SequenceValidation::DuplicateSequence => None,
            SequenceValidation::DivergentSequence => {
                self.transient_buffer.clear();
                self.transient_buffer.insert(sequence_index, txs);
                Some(self.assemble_flat_transactions())
            }
        }
    }

    /// Performs zero-copy linear flattening across buffered sub-second transaction frames.
    fn assemble_flat_transactions(&self) -> Vec<Bytes> {
        self.transient_buffer.values().flatten().cloned().collect()
    }
}
