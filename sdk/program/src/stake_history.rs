//! A type to hold data for the [`StakeHistory` sysvar][sv].
//!
//! [sv]: https://docs.solanalabs.com/runtime/sysvars#stakehistory
//!
//! The sysvar ID is declared in [`sysvar::stake_history`].
//!
//! [`sysvar::stake_history`]: crate::sysvar::stake_history

pub use crate::clock::Epoch;
use {
    crate::{program_error::ProgramError, sysvar::SysvarId},
    std::{ops::Deref, sync::Arc},
};

pub const MAX_ENTRIES: usize = 512; // it should never take as many as 512 epochs to warm up or cool down

#[repr(C)]
#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone)]
pub struct StakeHistoryEntry {
    pub effective: u64,    // effective stake at this epoch
    pub activating: u64,   // sum of portion of stakes not fully warmed up
    pub deactivating: u64, // requested to be cooled down, not fully deactivated yet
}

impl StakeHistoryEntry {
    pub fn with_effective(effective: u64) -> Self {
        Self {
            effective,
            ..Self::default()
        }
    }

    pub fn with_effective_and_activating(effective: u64, activating: u64) -> Self {
        Self {
            effective,
            activating,
            ..Self::default()
        }
    }

    pub fn with_deactivating(deactivating: u64) -> Self {
        Self {
            effective: deactivating,
            deactivating,
            ..Self::default()
        }
    }
}

impl std::ops::Add for StakeHistoryEntry {
    type Output = StakeHistoryEntry;
    fn add(self, rhs: StakeHistoryEntry) -> Self::Output {
        Self {
            effective: self.effective.saturating_add(rhs.effective),
            activating: self.activating.saturating_add(rhs.activating),
            deactivating: self.deactivating.saturating_add(rhs.deactivating),
        }
    }
}

#[repr(C)]
#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone)]
pub struct StakeHistory(Vec<(Epoch, StakeHistoryEntry)>);

impl StakeHistory {
    pub fn get(&self, epoch: Epoch) -> Option<&StakeHistoryEntry> {
        self.binary_search_by(|probe| epoch.cmp(&probe.0))
            .ok()
            .map(|index| &self[index].1)
    }

    pub fn add(&mut self, epoch: Epoch, entry: StakeHistoryEntry) {
        match self.binary_search_by(|probe| epoch.cmp(&probe.0)) {
            Ok(index) => (self.0)[index] = (epoch, entry),
            Err(index) => (self.0).insert(index, (epoch, entry)),
        }
        (self.0).truncate(MAX_ENTRIES);
    }
}

impl Deref for StakeHistory {
    type Target = Vec<(Epoch, StakeHistoryEntry)>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// we do not provide Default because this requires the real current epoch
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StakeHistorySyscall(Epoch);

impl StakeHistorySyscall {
    pub fn new(current_epoch: Epoch) -> Result<Self, ProgramError> {
        if current_epoch > 0 {
            Ok(Self(current_epoch))
        } else {
            Err(ProgramError::InvalidAccountData)
        }
    }
}

pub trait StakeHistoryGetEntry {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry>;
}

impl StakeHistoryGetEntry for StakeHistory {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry> {
        self.binary_search_by(|probe| epoch.cmp(&probe.0))
            .ok()
            .map(|index| self[index].1.clone())
    }
}

// required for SysvarCache
impl StakeHistoryGetEntry for Arc<StakeHistory> {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry> {
        self.deref().get_entry(epoch)
    }
}

// precompute so we can statically allocate buffer
const EPOCH_AND_ENTRY_SERIALIZED_SIZE: u64 = 32;

impl StakeHistoryGetEntry for StakeHistorySyscall {
    fn get_entry(&self, target_epoch: Epoch) -> Option<StakeHistoryEntry> {
        let current_epoch = self.0;
        let newest_historical_epoch = current_epoch.checked_sub(1)?;
        let oldest_historical_epoch = newest_historical_epoch.saturating_sub(MAX_ENTRIES as u64);

        // target epoch is before the beginning of time; this is a user error
        if target_epoch == 0 {
            return None;
        }

        // target epoch is old enough to have fallen off history; presume fully active/deactive
        if target_epoch < oldest_historical_epoch {
            return None;
        }

        // epoch delta is how many epoch-entries we offset in the stake history vector, which may be zero
        // None means target epoch is current or in the future; this is a user error
        let epoch_delta = newest_historical_epoch.checked_sub(target_epoch)?;

        // offset is the number of bytes to our desired entry, including eight for vector length
        let offset = epoch_delta
            .checked_mul(EPOCH_AND_ENTRY_SERIALIZED_SIZE)?
            .checked_add(std::mem::size_of::<u64>() as u64)?;

        let id_addr = StakeHistory::id().0.as_ptr();
        let mut entry_buf = [0; EPOCH_AND_ENTRY_SERIALIZED_SIZE as usize];
        let entry_buf_addr = &mut entry_buf as *mut _ as *mut u8;

        #[cfg(target_os = "solana")]
        let result = unsafe {
            crate::syscalls::sol_get_sysvar(
                id_addr,
                entry_buf_addr,
                offset,
                EPOCH_AND_ENTRY_SERIALIZED_SIZE,
            )
        };

        #[cfg(not(target_os = "solana"))]
        let result = crate::program_stubs::sol_get_sysvar(
            id_addr,
            entry_buf_addr,
            offset,
            EPOCH_AND_ENTRY_SERIALIZED_SIZE,
        );

        match result {
            crate::entrypoint::SUCCESS => {
                let (entry_epoch, entry) =
                    bincode::deserialize::<(Epoch, StakeHistoryEntry)>(&entry_buf).ok()?;

                // this would only fail if stake history skipped an epoch or the binary format of the sysvar changed
                assert_eq!(entry_epoch, target_epoch);

                Some(entry)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precompute_entry_size() {
        let mut stake_history = StakeHistory::default();
        stake_history.add(
            1,
            StakeHistoryEntry {
                activating: 1,
                ..StakeHistoryEntry::default()
            },
        );

        let epoch_entry = stake_history.0.into_iter().next().unwrap();
        assert_eq!(
            bincode::serialized_size(&epoch_entry).unwrap(),
            EPOCH_AND_ENTRY_SERIALIZED_SIZE
        );
    }

    #[test]
    fn test_stake_history() {
        let mut stake_history = StakeHistory::default();

        for i in 0..MAX_ENTRIES as u64 + 1 {
            stake_history.add(
                i,
                StakeHistoryEntry {
                    activating: i,
                    ..StakeHistoryEntry::default()
                },
            );
        }
        assert_eq!(stake_history.len(), MAX_ENTRIES);
        assert_eq!(stake_history.iter().map(|entry| entry.0).min().unwrap(), 1);
        assert_eq!(stake_history.get(0), None);
        assert_eq!(
            stake_history.get(1),
            Some(&StakeHistoryEntry {
                activating: 1,
                ..StakeHistoryEntry::default()
            })
        );
    }
}
