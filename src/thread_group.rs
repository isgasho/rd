use crate::session::session_inner::session_inner::SessionInner;
use crate::session::Session;
use crate::task_set::TaskSet;
use crate::taskish_uid::ThreadGroupUid;
use crate::wait_status::WaitStatus;
use libc::pid_t;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

pub type ThreadGroupSharedPtr = Rc<RefCell<ThreadGroup>>;

/// Tracks a group of tasks with an associated ID, set from the
/// original "thread group leader", the child of |fork()| which became
/// the ancestor of all other threads in the group.  Each constituent
/// task must own a reference to this.
///
/// Note: We DONT want to derive Clone.
pub struct ThreadGroup {
    /// These are the various tasks (dyn Task) that are part of the
    /// thread group.
    tasks: TaskSet,
    pub tgid: pid_t,
    pub real_tgid: pid_t,
    pub real_tgid_own_namespace: pid_t,

    pub exit_status: WaitStatus,

    /// We don't allow tasks to make themselves undumpable. If they try,
    /// record that here and lie about it if necessary.
    pub dumpable: bool,

    /// Whether this thread group has execed
    pub execed: bool,

    /// True when a task in the task-group received a SIGSEGV because we
    /// couldn't push a signal handler frame. Only used during recording.
    pub received_sigframe_sigsegv: bool,

    /// private fields
    /// In rr, nullptr is used to indicate no session.
    session_interface: Option<*mut dyn Session>,
    /// Parent ThreadGroup, or None if it's not a tracee (rd or init).
    /// Different from rr where nullptr is used.
    parent_: Option<*mut ThreadGroup>,

    children_: HashSet<*mut ThreadGroup>,

    serial: u32,
}

impl Drop for ThreadGroup {
    fn drop(&mut self) {
        unimplemented!()
    }
}

impl Deref for ThreadGroup {
    type Target = TaskSet;

    fn deref(&self) -> &Self::Target {
        &self.tasks
    }
}

impl DerefMut for ThreadGroup {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tasks
    }
}

/// Tracks a group of tasks with an associated ID, set from the
/// original "thread group leader", the child of |fork()| which became
/// the ancestor of all other threads in the group.  Each constituent
/// task must own a reference to this.
impl ThreadGroup {
    pub fn new(
        session: &SessionInner,
        parent: Option<&ThreadGroup>,
        tgid: pid_t,
        real_tgid: pid_t,
        real_tgid_own_namespace: pid_t,
        serial: u32,
    ) -> ThreadGroup {
        unimplemented!()
    }

    /// Mark the members of this thread group as "unstable",
    /// meaning that even though a task may look runnable, it
    /// actually might not be.  (And so |waitpid(-1)| should be
    /// used to schedule the next task.)
    ///
    /// This is needed to handle the peculiarities of mass Task
    /// death at exit_group() and upon receiving core-dumping
    /// signals.  The reason it's needed is easier to understand if
    /// you keep in mind that the "main loop" of ptrace tracers is
    /// /supposed/ to look like
    ///
    ///   while (true) {
    ///     int tid = waitpid(-1, ...);
    ///     // do something with tid
    ///     ptrace(tid, PTRACE_SYSCALL, ...);
    ///   }
    ///
    /// That is, the tracer is supposed to let the kernel schedule
    /// threads and then respond to notifications generated by the
    /// kernel.
    ///
    /// Obviously this isn't how rd's recorder loop looks, because,
    /// among other things, rd has to serialize thread execution.
    /// Normally this isn't much of a problem.  However, mass task
    /// death is an exception.  What happens at a mass task death
    /// is a sequence of events like the following
    ///
    ///  1. A task calls exit_group() or is sent a core-dumping
    ///     signal.
    ///  2. rd receives a PTRACE_EVENT_EXIT notification for the
    ///     task.
    ///  3. rd detaches from the dying/dead task.
    ///  4. Successive calls to waitpid(-1) generate additional
    ///     PTRACE_EVENT_EXIT notifications for each also-dead task
    ///     in the original task's thread group.  Repeat (2) / (3)
    ///     for each notified task.
    ///
    /// So why destabilization?  After (2), rd can't block on the
    /// task shutting down (|waitpid(tid)|), because the kernel
    /// harvests the LWPs of the dying thread group in an unknown
    /// order (which we shouldn't assume, even if we could guess
    /// it).  If rd blocks on the task harvest, it will (usually)
    /// deadlock.
    ///
    /// And because rd doesn't know the order of tasks that will be
    /// reaped, it doesn't know which of the dying tasks to
    /// "schedule".  If it guesses and blocks on another task in
    /// the group's status-change, it will (usually) deadlock.
    ///
    /// So destabilizing a thread group, from rd's perspective, means
    /// handing scheduling control back to the kernel and not
    /// trying to harvest tasks before detaching from them.
    ///
    /// NB: an invariant of rd scheduling is that all process
    /// status changes happen as a result of rd resuming the
    /// execution of a task.  This is required to keep tracees in
    /// known states, preventing events from happening "behind rd's
    /// back".  However, destabilizing a thread group means that
    /// these kinds of changes are possible, in theory.
    ///
    /// Currently, instability is a one-way street; it's only used
    /// needed for death signals and exit_group().
    pub fn destabilize(&self) {
        unimplemented!()
    }

    /// @TODO avoid this unsafe somehow?
    pub fn session(&self) -> &dyn Session {
        unsafe { self.session_interface.unwrap().as_ref() }.unwrap()
    }
    /// @TODO Should we have &mut self here?
    pub fn session_mut(&self) -> &mut dyn Session {
        unsafe { self.session_interface.unwrap().as_mut() }.unwrap()
    }
    pub fn forget_session(&mut self) {
        self.session_interface = None;
    }

    pub fn parent(&self) -> Option<&ThreadGroup> {
        unimplemented!()
    }
    pub fn children(&self) -> HashSet<&ThreadGroup> {
        unimplemented!()
    }

    pub fn tguid(&self) -> ThreadGroupUid {
        unimplemented!()
    }
}
