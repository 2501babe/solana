//! History of stake activations and de-activations.
//!
//! The _stake history sysvar_ provides access to the [`StakeHistory`] type.
//!
//! The [`Sysvar::get`] method always returns
//! [`ProgramError::UnsupportedSysvar`], and in practice the data size of this
//! sysvar is too large to process on chain. One can still use the
//! [`SysvarId::id`], [`SysvarId::check_id`] and [`Sysvar::size_of`] methods in
//! an on-chain program, and it can be accessed off-chain through RPC.
//!
//! [`ProgramError::UnsupportedSysvar`]: crate::program_error::ProgramError::UnsupportedSysvar
//! [`SysvarId::id`]: crate::sysvar::SysvarId::id
//! [`SysvarId::check_id`]: crate::sysvar::SysvarId::check_id
//!
//! # Examples
//!
//! Calling via the RPC client:
//!
//! ```
//! # use solana_program::example_mocks::solana_sdk;
//! # use solana_program::example_mocks::solana_rpc_client;
//! # use solana_sdk::account::Account;
//! # use solana_rpc_client::rpc_client::RpcClient;
//! # use solana_sdk::sysvar::stake_history::{self, StakeHistory};
//! # use anyhow::Result;
//! #
//! fn print_sysvar_stake_history(client: &RpcClient) -> Result<()> {
//! #   client.set_get_account_response(stake_history::ID, Account {
//! #       lamports: 114979200,
//! #       data: vec![0, 0, 0, 0, 0, 0, 0, 0],
//! #       owner: solana_sdk::system_program::ID,
//! #       executable: false,
//! #       rent_epoch: 307,
//! #   });
//! #
//!     let stake_history = client.get_account(&stake_history::ID)?;
//!     let data: StakeHistory = bincode::deserialize(&stake_history.data)?;
//!
//!     Ok(())
//! }
//! #
//! # let client = RpcClient::new(String::new());
//! # print_sysvar_stake_history(&client)?;
//! #
//! # Ok::<(), anyhow::Error>(())
//! ```

pub use crate::stake_history::StakeHistory;
use crate::{
    clock::Epoch,
    program_error::ProgramError,
    stake_history::{StakeHistoryEntry, StakeHistoryGetEntry, MAX_ENTRIES},
    sysvar::{get_sysvar, Sysvar, SysvarId},
};

crate::declare_sysvar_id!("SysvarStakeHistory1111111111111111111111111", StakeHistory);

impl Sysvar for StakeHistory {
    // override
    fn size_of() -> usize {
        // hard-coded so that we don't have to construct an empty
        16392 // golden, update if MAX_ENTRIES changes
    }
}

// we do not provide Default because this requires the real current epoch
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StakeHistorySysvar(Epoch);

impl StakeHistorySysvar {
    pub fn new(current_epoch: Epoch) -> Result<Self, ProgramError> {
        if current_epoch > 0 {
            Ok(Self(current_epoch))
        } else {
            Err(ProgramError::InvalidAccountData)
        }
    }
}

// precompute so we can statically allocate buffer
const EPOCH_AND_ENTRY_SERIALIZED_SIZE: u64 = 32;

impl StakeHistoryGetEntry for StakeHistorySysvar {
    fn get_entry(&self, target_epoch: Epoch) -> Option<StakeHistoryEntry> {
        let current_epoch = self.0;
        let newest_historical_epoch = current_epoch.checked_sub(1)?;
        let oldest_historical_epoch = current_epoch.saturating_sub(MAX_ENTRIES as u64);

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

        let mut entry_buf = [0; EPOCH_AND_ENTRY_SERIALIZED_SIZE as usize];
        let result = get_sysvar(
            &mut entry_buf,
            &StakeHistory::id(),
            offset,
            EPOCH_AND_ENTRY_SERIALIZED_SIZE,
        );

        match result {
            Ok(()) => {
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
    use {
        super::*,
        crate::{
            entrypoint::SUCCESS,
            program_stubs::{set_syscall_stubs, SyscallStubs},
            stake_history::*,
        },
    };

    struct MockStakeHistorySyscall {
        stake_history: StakeHistory,
    }

    impl SyscallStubs for MockStakeHistorySyscall {
        #[allow(clippy::arithmetic_side_effects)]
        fn sol_get_sysvar(
            &self,
            _sysvar_id_addr: *const u8,
            var_addr: *mut u8,
            offset: u64,
            length: u64,
        ) -> u64 {
            let data = bincode::serialize(&self.stake_history).unwrap();
            let slice = unsafe { std::slice::from_raw_parts_mut(var_addr, length as usize) };
            slice.copy_from_slice(&data[offset as usize..(offset + length) as usize]);
            SUCCESS
        }
    }

    fn mock_get_sysvar_syscall(stake_history: StakeHistory) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            set_syscall_stubs(Box::new(MockStakeHistorySyscall { stake_history }));
        });
    }

    #[test]
    fn test_size_of() {
        let mut stake_history = StakeHistory::default();
        for i in 0..MAX_ENTRIES as u64 {
            stake_history.add(
                i,
                StakeHistoryEntry {
                    activating: i,
                    ..StakeHistoryEntry::default()
                },
            );
        }

        assert_eq!(
            bincode::serialized_size(&stake_history).unwrap() as usize,
            StakeHistory::size_of()
        );

        let stake_history_inner: Vec<(Epoch, StakeHistoryEntry)> =
            bincode::deserialize(&bincode::serialize(&stake_history).unwrap()).unwrap();
        let epoch_entry = stake_history_inner.into_iter().next().unwrap();

        assert_eq!(
            bincode::serialized_size(&epoch_entry).unwrap(),
            EPOCH_AND_ENTRY_SERIALIZED_SIZE
        );
    }

    #[test]
    fn test_stake_history_get_entry() {
        let unique_entry_for_epoch = |epoch: u64| StakeHistoryEntry {
            activating: epoch % 2,
            deactivating: epoch % 3,
            effective: epoch % 5,
        };

        let current_epoch = MAX_ENTRIES as u64 + 2;

        // make a stake history object with at least one valid entry that has expired
        let mut stake_history = StakeHistory::default();
        for i in 0..current_epoch {
            stake_history.add(i, unique_entry_for_epoch(i));
        }
        assert_eq!(stake_history.len(), MAX_ENTRIES);
        assert_eq!(stake_history.iter().map(|entry| entry.0).min().unwrap(), 2);

        // set up sol_get_sysvar
        mock_get_sysvar_syscall(stake_history.clone());

        // make a syscall interface object
        let stake_history_sysvar = StakeHistorySysvar::new(current_epoch).unwrap();

        // now test the stake history interfaces

        assert_eq!(stake_history.get(0), None);
        assert_eq!(stake_history.get(1), None);
        assert_eq!(stake_history.get(current_epoch), None);

        assert_eq!(stake_history.get_entry(0), None);
        assert_eq!(stake_history.get_entry(1), None);
        assert_eq!(stake_history.get_entry(current_epoch), None);

        assert_eq!(stake_history_sysvar.get_entry(0), None);
        assert_eq!(stake_history_sysvar.get_entry(1), None);
        assert_eq!(stake_history_sysvar.get_entry(current_epoch), None);

        for i in 2..current_epoch {
            let entry = Some(unique_entry_for_epoch(i));

            assert_eq!(stake_history.get(i), entry.as_ref(),);

            assert_eq!(stake_history.get_entry(i), entry,);

            assert_eq!(stake_history_sysvar.get_entry(i), entry,);
        }
    }
}
