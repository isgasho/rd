#[cfg(feature = "verify_syscall_numbers")]
include!(concat!(
    env!("OUT_DIR"),
    "/check_syscall_numbers_generated.rs"
));
use crate::{
    arch::Architecture,
    auto_remote_syscalls::{AutoRemoteSyscalls, PreserveContents::PreserveContents},
    kernel_abi::{
        common::preload_interface::syscallbuf_hdr,
        is_rdcall_notify_syscall_hook_exit_syscall,
        is_restart_syscall_syscall,
        is_write_syscall,
        CloneTLSType,
        SupportedArch,
    },
    kernel_metadata::syscall_name,
    log::LogLevel::LogDebug,
    remote_ptr::RemotePtr,
    session::{
        address_space::{kernel_mapping::KernelMapping, MappingFlags},
        replay_session::{
            ReplaySession,
            ReplayTraceStep,
            ReplayTraceStepData,
            ReplayTraceStepSyscall,
            ReplayTraceStepType,
        },
        task::{
            common::write_val_mem,
            replay_task::ReplayTask,
            task_inner::{ResumeRequest, TicksRequest, WaitRequest},
            Task,
        },
    },
    trace::{
        trace_frame::FrameTime,
        trace_stream,
        trace_stream::MappedData,
        trace_task_event::{TraceTaskEvent, TraceTaskEventType},
    },
    util::{clone_flags_to_task_flags, extract_clone_parameters, CloneParameters},
    wait_status::WaitStatus,
};
use libc::{
    pid_t,
    CLONE_CHILD_CLEARTID,
    CLONE_NEWCGROUP,
    CLONE_NEWIPC,
    CLONE_NEWNET,
    CLONE_NEWNS,
    CLONE_NEWPID,
    CLONE_NEWUSER,
    CLONE_NEWUTS,
    CLONE_UNTRACED,
    CLONE_VFORK,
    CLONE_VM,
    ENOSYS,
};
use nix::sys::mman::{MapFlags, ProtFlags};
use std::{
    cmp::min,
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStringExt,
};

/// Proceeds until the next system call, which is being executed.
///
/// DIFF NOTE: Params maybe_expect_syscallno2 and maybe_new_tid and treatment slightly different.
fn __ptrace_cont(
    t: &mut ReplayTask,
    resume_how: ResumeRequest,
    syscall_arch: SupportedArch,
    expect_syscallno: i32,
    maybe_expect_syscallno2: Option<i32>,
    maybe_new_tid: Option<pid_t>,
) {
    maybe_expect_syscallno2.map(|n| debug_assert!(n >= 0));
    maybe_new_tid.map(|n| assert!(n > 0));
    let new_tid = maybe_new_tid.unwrap_or(-1);
    let expect_syscallno2 = maybe_expect_syscallno2.unwrap_or(-1);
    t.resume_execution(
        resume_how,
        WaitRequest::ResumeNonblocking,
        TicksRequest::ResumeNoTicks,
        None,
    );
    loop {
        if t.wait_unexpected_exit() {
            break;
        }
        let mut raw_status: i32 = 0;
        // Do our own waitpid instead of calling Task::wait() so we can detect and
        // handle tid changes due to off-main-thread execve.
        // When we're expecting a tid change, we can't pass a tid here because we
        // don't know which tid to wait for.
        // Passing the original tid seems to cause a hang in some kernels
        // (e.g. 4.10.0-19-generic) if the tid change races with our waitpid
        let ret = unsafe { libc::waitpid(new_tid, &mut raw_status, libc::__WALL) };
        ed_assert!(t, ret >= 0);
        if ret == new_tid {
            // Check that we only do this once
            ed_assert!(t, t.tid != new_tid);
            // Update the serial as if this task was really created by cloning the old task.
            t.set_real_tid_and_update_serial(new_tid);
        }
        ed_assert!(t, ret == t.tid);
        t.did_waitpid(WaitStatus::new(raw_status));

        // DIFF NOTE: @TODO The `if` statement logic may create a slight divergence from rr.
        // May need to think about this more deeply and make sure this will work like rr.
        if t.status().maybe_stop_sig().is_sig()
            && ReplaySession::is_ignored_signal(t.status().maybe_stop_sig().unwrap_sig())
        {
            t.resume_execution(
                resume_how,
                WaitRequest::ResumeNonblocking,
                TicksRequest::ResumeNoTicks,
                None,
            );
        } else {
            break;
        }
    }

    ed_assert!(
        t,
        !t.maybe_stop_sig().is_sig(),
        "Expected no pending signal, but got: {}",
        t.maybe_stop_sig().unwrap_sig()
    );

    // check if we are synchronized with the trace -- should never fail
    let current_syscall = t.regs_ref().original_syscallno() as i32;
    // DIFF NOTE: Minor differences arising out of maybe_dump_written_string() behavior.
    ed_assert!(
        t,
        current_syscall == expect_syscallno || current_syscall == expect_syscallno2,
        "Should be at {}, but instead at {} ({:?})",
        syscall_name(expect_syscallno, syscall_arch),
        syscall_name(current_syscall, syscall_arch),
        maybe_dump_written_string(t)
    );
}

/// DIFF NOTE: In rd we're returning a `None` if this was not a write syscall
fn maybe_dump_written_string(t: &mut ReplayTask) -> Option<OsString> {
    if !is_write_syscall(t.regs_ref().original_syscallno() as i32, t.arch()) {
        return None;
    }
    let len = min(1000, t.regs_ref().arg3());
    let mut buf = Vec::<u8>::with_capacity(len);
    buf.resize(len, 0u8);
    // DIFF NOTE: Here we're actually expecting there to be no Err(_), hence the unwrap()
    let nread = t
        .read_bytes_fallible(t.regs_ref().arg2().into(), &mut buf)
        .unwrap();
    buf.truncate(nread);
    Some(OsString::from_vec(buf))
}

fn init_scratch_memory(t: &mut ReplayTask, km: &KernelMapping, data: &trace_stream::MappedData) {
    ed_assert!(t, data.source == trace_stream::MappedDataSource::SourceZero);

    t.scratch_ptr = km.start();
    t.scratch_size = km.size();
    let sz = t.scratch_size;
    let scratch_ptr = t.scratch_ptr;
    // Make the scratch buffer read/write during replay so that
    // preload's sys_read can use it to buffer cloned data.
    ed_assert!(
        t,
        km.prot()
            .contains(ProtFlags::PROT_READ | ProtFlags::PROT_WRITE)
    );
    ed_assert!(
        t,
        km.flags()
            .contains(MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS)
    );

    {
        {
            let mut remote = AutoRemoteSyscalls::new(t);
            remote.infallible_mmap_syscall(
                Some(scratch_ptr),
                sz,
                km.prot(),
                km.flags() | MapFlags::MAP_FIXED,
                -1,
                0,
            );
        }
        t.vm_mut().map(
            t,
            t.scratch_ptr,
            sz,
            km.prot(),
            km.flags(),
            0,
            OsStr::new(""),
            KernelMapping::NO_DEVICE,
            KernelMapping::NO_INODE,
            None,
            Some(&km),
            None,
            None,
            None,
        );
    }
    t.setup_preload_thread_locals();
}

/// If scratch data was incidentally recorded for the current desched'd
/// but write-only syscall, then do a no-op restore of that saved data
/// to keep the trace in sync.
///
/// Syscalls like `write()` that may-block and are wrapped in the
/// preload library can be desched'd.  When this happens, we save the
/// syscall record's "extra data" as if it were normal scratch space,
/// since it's used that way in effect.  But syscalls like `write()`
/// that don't actually use scratch space don't ever try to restore
/// saved scratch memory during replay.  So, this helper can be used
/// for that class of syscalls.
fn maybe_noop_restore_syscallbuf_scratch(t: &mut ReplayTask) {
    if t.is_in_untraced_syscall() {
        // Untraced syscalls always have t's arch
        log!(
            LogDebug,
            "  noop-restoring scratch for write-only desched'd {}",
            syscall_name(t.regs_ref().original_syscallno() as i32, t.arch())
        );
        t.set_data_from_trace();
    }
}

fn read_task_trace_event(t: &ReplayTask, task_event_type: TraceTaskEventType) -> TraceTaskEvent {
    let mut tte: Option<TraceTaskEvent>;
    let mut time: FrameTime = 0;
    let shr_ptr = t.session();

    let mut tr = shr_ptr.as_replay().unwrap().trace_reader_mut();
    loop {
        tte = tr.read_task_event(Some(&mut time));
        if tte.is_none() {
            ed_assert!(
                t,
                false,
                "Unable to find TraceTaskEvent; trace is corrupt (did you kill -9 rd?)"
            )
        }

        if time >= t.current_frame_time() && tte.as_ref().unwrap().event_type() == task_event_type {
            break;
        }
    }
    ed_assert!(t, time == t.current_frame_time());
    tte.unwrap()
}

fn prepare_clone<Arch: Architecture>(t: &mut ReplayTask) {
    let trace_frame = t.current_trace_frame();
    let trace_frame_regs = trace_frame.regs_ref().clone();
    let syscall_event = trace_frame.event().syscall_event();
    let syscall_event_arch = syscall_event.arch();

    // We're being called with the syscall entry event, so we can't inspect the result
    // of the syscall exit to see whether the clone succeeded (that event can happen
    // much later, even after the spawned task has run).
    if syscall_event.failed_during_preparation {
        // creation failed, nothing special to do
        return;
    }
    drop(trace_frame);

    let mut r = t.regs_ref().clone();
    let mut sys: i32 = r.original_syscallno() as i32;
    let mut flags: i32 = 0;
    if Arch::CLONE == sys {
        // If we allow CLONE_UNTRACED then the child would escape from rr control
        // and we can't allow that.
        // Block CLONE_CHILD_CLEARTID because we'll emulate that ourselves.
        // Block CLONE_VFORK for the reasons below.
        // Block CLONE_NEW* from replay, any effects it had were dealt with during
        // recording.
        let disallowed_clone_flags = CLONE_UNTRACED
            | CLONE_CHILD_CLEARTID
            | CLONE_VFORK
            | CLONE_NEWIPC
            | CLONE_NEWNET
            | CLONE_NEWNS
            | CLONE_NEWPID
            | CLONE_NEWUSER
            | CLONE_NEWUTS
            | CLONE_NEWCGROUP;
        flags = r.arg1() as i32 & !disallowed_clone_flags;
        r.set_arg1(flags as usize);
    } else if Arch::VFORK == sys {
        // We can't perform a real vfork, because the kernel won't let the vfork
        // parent return from the syscall until the vfork child has execed or
        // exited, and it is an invariant of replay that tasks are not in the kernel
        // except when we need them to execute a specific syscall on rr's behalf.
        // So instead we do a regular fork but use the CLONE_VM flag to share
        // address spaces between the parent and child. That's just like a vfork
        // except the parent is immediately runnable. This is no problem for replay
        // since we follow the recorded schedule in which the vfork parent did not
        // run until the vfork child exited.
        sys = Arch::CLONE;
        flags = CLONE_VM;
        r.set_arg1(flags as usize);
        r.set_arg2(0);
    }
    r.set_syscallno(sys as isize);
    r.set_ip(r.ip().decrement_by_syscall_insn_length(r.arch()));
    t.set_regs(&r);
    let entry_regs = r.clone();

    // Run; we will be interrupted by PTRACE_EVENT_CLONE/FORK/VFORK.
    __ptrace_cont(
        t,
        ResumeRequest::ResumeCont,
        Arch::arch(),
        sys as i32,
        None,
        None,
    );

    let mut new_tid: Option<pid_t> = None;
    while !t.clone_syscall_is_complete(&mut new_tid, Arch::arch()) {
        // clone() calls sometimes fail with -EAGAIN due to load issues or
        // whatever. We need to retry the system call until it succeeds. Reset
        // state to try the syscall again.
        ed_assert!(
            t,
            t.regs_ref().syscall_result_signed() == -libc::EAGAIN as isize
        );
        t.set_regs(&entry_regs);
        __ptrace_cont(
            t,
            ResumeRequest::ResumeCont,
            Arch::arch(),
            sys as i32,
            None,
            None,
        );
    }

    // Get out of the syscall
    __ptrace_cont(
        t,
        ResumeRequest::ResumeSyscall,
        Arch::arch(),
        sys as i32,
        None,
        None,
    );

    ed_assert!(
        t,
        t.maybe_ptrace_event().is_ptrace_event(),
        "Unexpected ptrace event while waiting for syscall exit; got {}",
        t.maybe_ptrace_event()
    );

    r = t.regs_ref().clone();
    // Restore the saved flags, to hide the fact that we may have
    // masked out CLONE_UNTRACED/CLONE_CHILD_CLEARTID or changed from vfork to
    // clone.
    r.set_arg1(trace_frame_regs.arg1());
    r.set_arg2(trace_frame_regs.arg2());
    // Pretend we're still in the system call
    r.set_syscall_result(-ENOSYS as usize);
    r.set_original_syscallno(trace_frame_regs.original_syscallno());
    t.set_regs(&r);
    t.canonicalize_regs(syscall_event_arch);

    // Dig the recorded tid out out of the trace. The tid value returned in
    // the recorded registers could be in a different pid namespace from rr's,
    // so we can't use it directly.
    let tte = read_task_trace_event(t, TraceTaskEventType::Clone);
    ed_assert!(
        t,
        tte.clone_variant().parent_tid() == t.rec_tid,
        "Expected tid {}, got {}",
        t.rec_tid,
        tte.clone_variant().parent_tid()
    );
    let rec_tid = tte.tid();

    let mut params: CloneParameters = Default::default();
    if Arch::CLONE as isize == t.regs_ref().original_syscallno() {
        params = extract_clone_parameters(t);
    }
    let shr_ptr = t.session();

    let new_task: &mut ReplayTask = shr_ptr
        .clone_task(
            t,
            clone_flags_to_task_flags(flags),
            params.stack,
            params.tls,
            params.ctid,
            new_tid.unwrap(),
            Some(rec_tid),
        )
        .as_replay_task_mut()
        .unwrap();

    if Arch::CLONE as isize == t.regs_ref().original_syscallno() {
        // FIXME: what if registers are non-null and contain an invalid address?
        t.set_data_from_trace();

        if Arch::CLONE_TLS_TYPE == CloneTLSType::UserDescPointer {
            t.set_data_from_trace();
            new_task.set_data_from_trace();
        } else {
            debug_assert!(Arch::CLONE_TLS_TYPE == CloneTLSType::PthreadStructurePointer);
        }
        new_task.set_data_from_trace();
        new_task.set_data_from_trace();
    }

    // Fix registers in new task
    let mut new_r = new_task.regs_ref().clone();
    let new_task_arch = new_r.arch();
    new_r.set_original_syscallno(trace_frame_regs.original_syscallno());
    new_r.set_arg1(trace_frame_regs.arg1());
    new_r.set_arg2(trace_frame_regs.arg2());
    new_task.set_regs(&new_r);
    new_task.canonicalize_regs(new_task_arch);

    if Arch::CLONE as isize != t.regs_ref().original_syscallno()
        || !(CLONE_VM as usize & r.arg1() == CLONE_VM as usize)
    {
        // It's hard to imagine a scenario in which it would
        // be useful to inherit breakpoints (along with their
        // refcounts) across a non-VM-sharing clone, but for
        // now we never want to do this.
        new_task.vm_mut().remove_all_breakpoints();
        new_task.vm_mut().remove_all_watchpoints();

        let mut remote = AutoRemoteSyscalls::new(new_task);
        for (&k, m) in t.vm().maps() {
            // Recreate any tracee-shared mappings
            if m.local_addr.is_some()
                && !m
                    .flags
                    .contains(MappingFlags::IS_THREAD_LOCALS | MappingFlags::IS_SYSCALLBUF)
            {
                remote.recreate_shared_mmap(k, Some(PreserveContents), None);
            }
        }
    }

    let mut data: MappedData = Default::default();
    let km: KernelMapping;
    {
        let shr_ptr = t.session();

        let replay_session = shr_ptr.as_replay().unwrap();
        km = replay_session
            .trace_reader_mut()
            .read_mapped_region(Some(&mut data), None, None, None, None)
            .unwrap();
    }

    init_scratch_memory(new_task, &km, &data);

    new_task.vm_mut().after_clone();
}

/// DIFF NOTE: This simply returns a ReplayTraceStep instead of modifying one.
pub fn rep_prepare_run_to_syscall(t: &mut ReplayTask) -> ReplayTraceStep {
    let step: ReplayTraceStep;
    let sys_num = t.current_trace_frame().event().syscall_event().number;
    let sys_arch = t.current_trace_frame().event().syscall_event().arch();
    let sys_name = t
        .current_trace_frame()
        .event()
        .syscall_event()
        .syscall_name();
    log!(LogDebug, "processing {} (entry)", sys_name);

    if is_restart_syscall_syscall(sys_num, sys_arch) {
        ed_assert!(t, t.tick_count() == t.current_trace_frame().ticks());
        let regs = t.current_trace_frame().regs_ref().clone();
        t.set_regs(&regs);
        t.apply_all_data_records_from_trace();
        step = ReplayTraceStep {
            action: ReplayTraceStepType::TstepRetire,
            data: Default::default(),
        };
        return step;
    }

    step = ReplayTraceStep {
        action: ReplayTraceStepType::TstepEnterSyscall,
        data: ReplayTraceStepData::Syscall(ReplayTraceStepSyscall {
            // @TODO Check again: is this what we want for arch?
            arch: sys_arch,
            number: sys_num,
        }),
    };

    // Don't let a negative incoming syscall number be treated as a real
    // system call that we assigned a negative number because it doesn't
    // exist in this architecture.
    if is_rdcall_notify_syscall_hook_exit_syscall(sys_num, sys_arch) {
        ed_assert!(t, !t.syscallbuf_child.is_null());
        let child_addr = RemotePtr::<u8>::cast(t.syscallbuf_child)
            + offset_of!(syscallbuf_hdr, notify_on_syscall_hook_exit);
        write_val_mem(t, child_addr, &1u8, None);
    }

    step
}

pub fn rep_process_syscall(_t: &ReplayTask, _step: &ReplayTraceStep) {
    unimplemented!()
}
