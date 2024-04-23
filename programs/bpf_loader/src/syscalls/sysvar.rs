use super::*;

fn get_sysvar<T: std::fmt::Debug + Sysvar + SysvarId + Clone>(
    sysvar: Result<T, InstructionError>,
    var_addr: u64,
    check_aligned: bool,
    memory_mapping: &mut MemoryMapping,
    invoke_context: &mut InvokeContext,
) -> Result<u64, Error> {
    consume_compute_meter(
        invoke_context,
        invoke_context
            .get_compute_budget()
            .sysvar_base_cost
            .saturating_add(size_of::<T>() as u64),
    )?;
    let var = translate_type_mut::<T>(memory_mapping, var_addr, check_aligned)?;

    // this clone looks unecessary, but it exists to zero out trailing alignment bytes
    // it is unclear whether this should ever matter
    // but there are tests using MemoryMapping that expect to see this
    // we preserve the previous behavior out of an abundance of caution
    *var = sysvar?.clone();

    Ok(SUCCESS)
}

declare_builtin_function!(
    /// Get a Clock sysvar
    SyscallGetClockSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_clock(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

declare_builtin_function!(
    /// Get a EpochSchedule sysvar
    SyscallGetEpochScheduleSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_epoch_schedule(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

declare_builtin_function!(
    /// Get a EpochRewards sysvar
    SyscallGetEpochRewardsSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_epoch_rewards(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

declare_builtin_function!(
    /// Get a Fees sysvar
    SyscallGetFeesSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        #[allow(deprecated)]
        {
            get_sysvar(
                invoke_context.get_sysvar_cache().get_fees(),
                var_addr,
                invoke_context.get_check_aligned(),
                memory_mapping,
                invoke_context,
            )
        }
    }
);

declare_builtin_function!(
    /// Get a Rent sysvar
    SyscallGetRentSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_rent(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

declare_builtin_function!(
    /// Get a Last Restart Slot sysvar
    SyscallGetLastRestartSlotSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        var_addr: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        get_sysvar(
            invoke_context.get_sysvar_cache().get_last_restart_slot(),
            var_addr,
            invoke_context.get_check_aligned(),
            memory_mapping,
            invoke_context,
        )
    }
);

declare_builtin_function!(
    /// Get a slice of a Sysvar in-memory representation
    SyscallGetSysvar,
    fn rust(
        invoke_context: &mut InvokeContext,
        sysvar_id_addr: u64,
        var_addr: u64,
        offset: u64,
        length: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Error> {
        let check_aligned = invoke_context.get_check_aligned();
        let ComputeBudget {
            sysvar_base_cost,
            cpi_bytes_per_unit,
            mem_op_base_cost,
            ..
        } = *invoke_context.get_compute_budget();

        consume_compute_meter(
            invoke_context,
            sysvar_base_cost
                .saturating_add(32_u64.div_ceil(cpi_bytes_per_unit))
                .saturating_add(std::cmp::max(
                    length.div_ceil(cpi_bytes_per_unit),
                    mem_op_base_cost,
                )),
        )?;

        let sysvar_id = translate_type::<Pubkey>(memory_mapping, sysvar_id_addr, check_aligned)?;
        let var = translate_slice_mut::<u8>(memory_mapping, var_addr, length, check_aligned)?;

        let cache = invoke_context.get_sysvar_cache();
        cache.read_sysvar_into(sysvar_id, length as usize, offset as usize, var)?;

        Ok(SUCCESS)
    }
);
