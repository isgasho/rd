use crate::trace::trace_frame::FrameTime;

lazy_static! {
    static ref FLAGS: Flags = init_flags();
}

/// When to generate or check memory checksums. One of CHECKSUM_NONE,
/// CHECKSUM_SYSCALL or CHECKSUM_ALL, or a positive integer representing the
/// event time at which to start checksumming.
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum Checksum {
    ChecksumNone,
    ChecksumSyscall,
    ChecksumAll,
    ChecksumAt(FrameTime),
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum DumpOn {
    DumpOnAll,
    DumpOnRdtsc,
    DumpOnNone,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum DumpAt {
    DumpAtNone,
    DumpAt(FrameTime),
}

#[derive(Clone)]
pub struct Flags {
    pub checksum: Checksum,
    pub dump_on: DumpOn,
    pub dump_at: DumpAt,
    /// Force rr to do some things that it otherwise wouldn't, for
    /// example launching an emergency debugger when the output
    /// doesn't seem to be a tty.
    pub force_things: bool,
    /// Mark the trace global time along with tracee writes to stdio.
    pub mark_stdio: bool,
    /// Check that cached mmaps match /proc/maps after each event.
    pub check_cached_maps: bool,
    /// Any warning or error that would be printed is treated as fatal
    pub fatal_errors_and_warnings: bool,
    /// Pretend CPUID faulting support doesn't exist
    pub disable_cpuid_faulting: bool,
    /// Don't listen for PTRACE_EVENT_EXIT events, to test how rr handles
    /// missing PTRACE_EVENT_EXITs.
    pub disable_ptrace_exit_events: bool,
    /// User override for architecture detection, e.g. when running under valgrind.
    pub forced_uarch: String,
    /// User override for the path to page files and other resources.
    pub resource_path: String,
}

impl Flags {
    pub fn get() -> &'static Flags {
        &*FLAGS
    }
}

pub fn init_flags() -> Flags {
    unimplemented!()
}
