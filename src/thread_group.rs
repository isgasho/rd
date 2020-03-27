use crate::log::LogLevel::LogDebug;
use crate::session::{SessionSharedPtr, SessionSharedWeakPtr};
use crate::task::Task;
use crate::taskish_uid::ThreadGroupUid;
use crate::wait_status::WaitStatus;
use crate::weak_ptr_set::WeakPtrSet;
use libc::pid_t;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};

pub type ThreadGroupSharedPtr = Rc<RefCell<ThreadGroup>>;
pub type ThreadGroupSharedWeakPtr = Weak<RefCell<ThreadGroup>>;
pub type ThreadGroupRef<'a> = Ref<'a, ThreadGroup>;
pub type ThreadGroupRefMut<'a> = RefMut<'a, ThreadGroup>;

/// Tracks a group of tasks with an associated ID, set from the
/// original "thread group leader", the child of `fork()` which became
/// the ancestor of all other threads in the group.  Each constituent
/// task must own a reference to this.
///
/// Note: We DONT want to derive Clone.
pub struct ThreadGroup {
    /// These are the various tasks (dyn Task) that are part of the
    /// thread group.
    tasks: WeakPtrSet<Box<dyn Task>>,
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
    /// However, in rd we always assume there is a session.
    /// The only place where session is removed is the forget_session() method in rr
    /// which we don't use.
    session_: SessionSharedWeakPtr,
    /// Parent ThreadGroup, or None if it's not a tracee (rd or init).
    /// Different from rr where nullptr is used.
    parent_: Option<ThreadGroupSharedWeakPtr>,

    children_: HashSet<ThreadGroupSharedWeakPtr>,

    serial: u32,
    weak_self: ThreadGroupSharedWeakPtr,
}

impl Drop for ThreadGroup {
    fn drop(&mut self) {
        unimplemented!()
    }
}

impl Deref for ThreadGroup {
    type Target = WeakPtrSet<Box<dyn Task>>;

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
/// original "thread group leader", the child of `fork()` which became
/// the ancestor of all other threads in the group.  Each constituent
/// task must own a reference to this.
impl ThreadGroup {
    pub fn new(
        session: SessionSharedWeakPtr,
        parent: Option<ThreadGroupSharedWeakPtr>,
        tgid: pid_t,
        real_tgid: pid_t,
        real_tgid_own_namespace: pid_t,
        serial: u32,
    ) -> ThreadGroupSharedPtr {
        let tg = ThreadGroup {
            tgid,
            real_tgid,
            real_tgid_own_namespace,
            dumpable: true,
            execed: false,
            received_sigframe_sigsegv: false,
            session_: session.clone(),
            parent_: parent,
            serial,
            tasks: Default::default(),
            exit_status: Default::default(),
            children_: Default::default(),
            weak_self: Weak::new(),
        };
        log!(
            LogDebug,
            "creating new thread group {} (real tgid:{})",
            tgid,
            real_tgid
        );

        let tg_shared = Rc::new(RefCell::new(tg));
        let tg_weak = Rc::downgrade(&tg_shared);
        tg_shared.borrow_mut().weak_self = tg_weak.clone();

        // @TODO
        //if parent.is_some() {
        //    parent.unwrap().upgrade().unwrap().borrow_mut().children_.insert(tg_weak.clone());
        //}
        session
            .upgrade()
            .unwrap()
            .borrow_mut()
            .on_create_tg(tg_weak);
        tg_shared
    }

    /// Mark the members of this thread group as "unstable",
    /// meaning that even though a task may look runnable, it
    /// actually might not be.  (And so `waitpid(-1)` should be
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
    /// task shutting down (`waitpid(tid)`), because the kernel
    /// harvests the LWPs (Light weight processes) of the dying thread group in an unknown
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
        log!(LogDebug, "destabilizing thread group {}", self.tgid);
        for t in self.iter() {
            t.borrow_mut().unstable = true;
            log!(LogDebug, "  destabilized task {}", t.borrow().tid);
        }
    }

    pub fn session(&self) -> SessionSharedPtr {
        self.session_.upgrade().unwrap()
    }

    pub fn parent(&self) -> Option<ThreadGroupSharedPtr> {
        self.parent_.as_ref().map(|wp| wp.upgrade().unwrap())
    }

    pub fn children(&self) -> &HashSet<ThreadGroupSharedWeakPtr> {
        &self.children_
    }

    pub fn tguid(&self) -> ThreadGroupUid {
        ThreadGroupUid::new_with(self.tgid, self.serial)
    }

    pub fn self_ptr(&self) -> ThreadGroupSharedWeakPtr {
        self.weak_self.clone()
    }
}
