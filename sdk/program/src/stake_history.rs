//! A type to hold data for the [`StakeHistory` sysvar][sv].
//!
//! [sv]: https://docs.solanalabs.com/runtime/sysvars#stakehistory
//!
//! The sysvar ID is declared in [`sysvar::stake_history`].
//!
//! [`sysvar::stake_history`]: crate::sysvar::stake_history

pub use crate::clock::Epoch;
use {crate::sysvar::SysvarId, std::{sync::Arc, ops::Deref}};

pub const MAX_ENTRIES: usize = 512; // it should never take as many as 512 epochs to warm up or cool down

#[repr(C)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone, AbiExample)]
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
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone, AbiExample)]
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

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct StakeHistorySyscall(Epoch);

impl StakeHistorySyscall {
    pub fn new(current_epoch: Epoch) -> Self {
        Self(current_epoch)
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

impl StakeHistoryGetEntry for StakeHistorySyscall {
    // HANA ok after three tries i finally am at one with the dao of this
    // we can do this in zero or one syscalls always, if we have the current epoch
    // length is invariant, its just bincode sizeof
    // and the first epoch is always one less than the current one
    // that means... if target gte current, return err
    // if current minus target (gt? gte?) 512 return None
    // great do i actually need a result type? i cant anyway, the callers dont return Result
    // anyway whatever use asserts for now
    // so we have determined our epoch is in-history
    // which means the offset is... lol
    // newest entry starts at 8. which means to get it we do
    // current - target - 1?
    // if its 500 and we want 499 then yea current - 1 - target = 0
    // multiply by 32 to get the tuple pointer. add 8
    fn get_entry(&self, target_epoch: Epoch) -> Option<StakeHistoryEntry> {
        let current_epoch = self.0;
        let newest_historical_epoch = current_epoch - 1;
        let oldest_historical_epoch = newest_historical_epoch.saturating_sub(MAX_ENTRIES as u64);

        // HANA im not sure this is the right thing to do. i would kind of prefer to panic
        // this should never happen and indicates a bug in the caller
        // but changing all callers to handle a Result seems less than ideal
        // returning an entry with 0 stake is also an option but a bad one
        // if a loop has advanced to the current epoch then it will spinlock if this happens
        // which, again, should never happen. but better to succeed or die
        let epoch_delta = match newest_historical_epoch.checked_sub(target_epoch) {
            Some(d) => {
                assert!(target_epoch > newest_historical_epoch);
                d
            },
            None => panic!("target epoch is in the future"),
        };

        // dunno about this either
        if target_epoch == 0 {
            panic!("target epoch is before the beginning of time");
        }

        // ok if max were 10 and newest is 12 then we have
        // 12 11 10 9 8 7 6 4 3 2
        // that means if current is 13 the oldest is 13 - 1 - max
        if target_epoch < oldest_historical_epoch {
            return None;
        }

        // XXX ok recap because i fell asleep
        // we we get our epoch range. newest is first in series, oldest is last
        // newer than newest is an error. older than oldest means we assume its fully active/deactive
        // then we calculate an index based on... distance from the newest?
        // if newest  is 500 and target is 497, delta is 3
        // 500 499 498 497 yep that index is correct as-is, since we already subtracted 1 for currenth
        let offset = epoch_delta * 32 + 8;
        let id_addr = StakeHistory::id().0.as_ptr();
        let mut entry_buf = [0; 32];
        let entry_buf_addr = &mut entry_buf as *mut _ as *mut u8;
        
        #[cfg(target_os = "solana")]
        let result = unsafe { crate::syscalls::sol_get_sysvar(id_addr, 32, offset, entry_buf_addr) };

        #[cfg(not(target_os = "solana"))]
        let result = crate::program_stubs::sol_get_sysvar(id_addr, 32, offset, entry_buf_addr);

        assert_eq!(result, crate::entrypoint::SUCCESS);
        let (entry_epoch, entry) = bincode::deserialize::<(Epoch, StakeHistoryEntry)>(&entry_buf).unwrap();
        assert_eq!(entry_epoch, target_epoch);

        Some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
