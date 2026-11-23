//! Supervision: the process, root and resource primitives one child process per case needs.
//!
//! The harness runs the corpus's own per-case body in a child of the same binary in a hidden mode;
//! only the comparison against the recorded baseline and the printing of the verdict happen in the
//! parent. The child's inputs are built, the engine runs and the output is canonicalised inside the
//! child, because the corpus builder and the canonicaliser are allocation paths of their own.
//!
//! The bounds are configuration ([`Bounds`]) with documented defaults, because a bound that cannot
//! be set small cannot be tested. `dev/README.md` ("Supervision and the bounds") is the one
//! statement of what is bounded, what is not, how the footprint ceiling was derived, the precedence
//! among the bounds, the holes that are stated rather than closed, the platform limits and the
//! helper binary's behaviours; this module implements that statement and does not repeat it.
//!
//! # The child protocol
//!
//! The child is the same test binary in a hidden mode (`--ignored --exact child --`, or
//! `--exact helper_child --` for the helper), selected before the ordinary argument loop. Its
//! arguments are a protocol with an arbitrary binary: `--case writer:<slug>` or
//! `--case reading:<name>`, `--engine crate|noop|helper`, `--root <path>` and
//! `--transcript <path>`. [`ChildInvocation::parse`] reads them here; `harness::run_invocation`
//! builds the engine, runs the named case and writes the transcript. The helper child
//! (`dev/helper.rs`) speaks the same protocol.

use crate::engine::EngineIdentity;
use anyhow::{Context as _, Result, bail};
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The physical footprint a case's process tree may reach. Derived from legitimate cases only;
/// `dev/README.md` records the largest one, the measurement and the derivation.
const MEMORY_BYTES: u64 = 2048 * 1024 * 1024;

/// The apparent size a file may reach. Above the engine's declared 200 MiB merge input bound, so the
/// cap is never what decides what a case may do; a smaller cap would kill a legitimate merge before
/// the engine ran.
const FILE_BYTES: u64 = 256 * 1024 * 1024;

/// The allocated bytes a case's directory may reach. `dev/README.md` states the two bases for it.
const DIR_BYTES: u64 = 1024 * 1024 * 1024;

/// The entries a case's directory may hold, counted recursively. `dev/README.md` states the two
/// bases for it.
const DIR_ENTRIES: usize = 256;

/// How often the poller samples the case's footprint and directory.
const POLL: Duration = Duration::from_millis(100);

/// The largest transcript the parent reads. A transcript above this is a failed verdict naming the
/// cap rather than a parent allocation that grows with a runaway's output.
const TRANSCRIPT_BYTES: usize = 4 * 1024 * 1024;

/// How old a leftover root must be before it is reclaimed. The claim, not the grace, is what keeps a
/// live root alive: the grace covers the moment between a root's creation and its claim, and a root
/// that was never claimed at all.
const GRACE: Duration = Duration::from_secs(60);

/// How much of a child's stream is kept. Only the tail is ever quoted.
const STREAM_TAIL_BYTES: usize = 8 * 1024;

/// How long the parent waits for a stream's drain after the child exits. A descendant holding the
/// pipe can keep it open; the thread is then detached, its buffer bounded.
const DRAIN_GRACE: Duration = Duration::from_secs(5);

/// How long the parent waits for a killed tree to be reaped before killing again.
const REAP_GRACE: Duration = Duration::from_secs(5);

/// How long one writer case may take under supervision: twice the 120 s a case's own run is bounded
/// to, so fixture construction and canonicalisation fit beside a case that uses its whole budget.
const WRITER_TIMEOUT: Duration = Duration::from_secs(240);

/// How long one reading may take under supervision: the two bounded engine calls (120 s each), plus
/// fixture construction and canonicalisation.
const READING_TIMEOUT: Duration = Duration::from_secs(330);

/// The marker every harness root carries. A root without one is never swept: it is not a root this
/// harness created.
pub const ROOT_MARKER: &str = ".document-gate-root";

/// The prefix every harness root carries: a root is a directory this harness named.
pub(crate) const ROOT_PREFIX: &str = "document-gate-";

/// The bounds a supervised case runs under.
#[derive(Clone, Debug)]
pub struct Bounds {
    pub memory_bytes: u64,
    pub file_bytes: u64,
    pub dir_bytes: u64,
    pub dir_entries: usize,
    pub poll: Duration,
    pub transcript_bytes: usize,
    pub grace: Duration,
    pub writer_timeout: Duration,
    pub reading_timeout: Duration,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            memory_bytes: MEMORY_BYTES,
            file_bytes: FILE_BYTES,
            dir_bytes: DIR_BYTES,
            dir_entries: DIR_ENTRIES,
            poll: POLL,
            transcript_bytes: TRANSCRIPT_BYTES,
            grace: GRACE,
            writer_timeout: WRITER_TIMEOUT,
            reading_timeout: READING_TIMEOUT,
        }
    }
}

/// Which engine a child builds for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Crate,
    Noop,
    /// The harness's own helper binary, which speaks the child protocol and runs a runaway, a
    /// hanging or ordinary work by case. Only the harness's tests use it.
    Helper,
}

impl EngineKind {
    /// The name the child is told. The helper binary ignores it and builds its own engine.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Crate => "crate",
            Self::Noop => "noop",
            Self::Helper => "helper",
        }
    }

    /// The engine a child argument names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "crate" => Some(Self::Crate),
            "noop" => Some(Self::Noop),
            "helper" => Some(Self::Helper),
            _ => None,
        }
    }
}

/// How one supervised run is configured.
#[derive(Clone, Debug)]
pub struct Supervision {
    pub engine: EngineKind,
    /// The executable that speaks the child protocol. The gate names itself; a test whose own
    /// executable is the test binary points this at the test binary or the helper.
    pub child: PathBuf,
    /// Arguments inserted before the child protocol's own, so the executable can be this same test
    /// binary in one of its hidden modes: `--ignored --exact child --`, or the same
    /// prefix naming `helper_child`.
    pub child_prefix: Vec<String>,
    pub bounds: Bounds,
    /// Bounds that replace the run's for one named case or reading. A bound that cannot be set small
    /// for one case cannot be tested without failing every other case on it.
    pub override_for: Option<(String, Bounds)>,
    /// Environment the child is given in addition to the parent's. The helper binary selects its
    /// behaviour from it, so a test never touches the process environment.
    pub env: Vec<(String, String)>,
    /// Whether the run keeps every transcript. The gate does not: the parent holds no transcript it
    /// has already judged. A test that compares transcripts asks for them.
    pub collect: bool,
}

impl Supervision {
    /// A run whose child is an ordinary program speaking the protocol as its first argument.
    #[must_use]
    pub fn new(engine: EngineKind, child: PathBuf) -> Self {
        Self {
            engine,
            child,
            child_prefix: Vec::new(),
            bounds: Bounds::default(),
            override_for: None,
            env: Vec::new(),
            collect: false,
        }
    }

    /// A run whose child is this test binary in a hidden mode: `prefix` selects that mode.
    #[must_use]
    pub fn with_prefix(engine: EngineKind, child: PathBuf, prefix: Vec<String>) -> Self {
        Self {
            child_prefix: prefix,
            ..Self::new(engine, child)
        }
    }

    /// The bounds one entry runs under: the run's, or the override for its name.
    pub(crate) fn bounds_for(&self, target: &ChildTarget) -> &Bounds {
        match &self.override_for {
            Some((name, bounds)) if name == target.name() => bounds,
            _ => &self.bounds,
        }
    }
}

static SIGNAL: AtomicI32 = AtomicI32::new(0);

/// Install the handler that records an interrupt, termination or hang-up signal.
///
/// The handler only stores the signal number: that is the one thing that is both necessary and
/// async-signal-safe. It does not kill, print, allocate, take locks or remove directories. The
/// ordinary path notices the flag within one poll interval and does the whole teardown: walk the
/// descendants, kill them, kill the child, reclaim the root, emit no further verdicts and exit with
/// the cut-short code. The handler covers the interrupt, termination and hang-up signals, because a
/// CI cancellation sends termination and not an interrupt.
///
/// Only the gate binary installs it. A handler installed in a long-lived test process would outlive
/// the run that wanted it, leaving that process deaf to a signal after the first supervised run.
pub fn install_signal_handler() {
    #[cfg(unix)]
    {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            unsafe extern "C" fn handler(signal: libc::c_int) {
                SIGNAL.store(signal, Ordering::SeqCst);
            }
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                // SAFETY: the handler stores one atomic and returns; the action is zeroed and its
                // mask is empty, so no other signal is blocked.
                unsafe {
                    let mut action: libc::sigaction = std::mem::zeroed();
                    action.sa_sigaction = handler as *const () as usize;
                    action.sa_flags = 0;
                    libc::sigemptyset(&mut action.sa_mask);
                    libc::sigaction(signal, &action, std::ptr::null_mut());
                }
            }
        });
    }
}

/// The signal that arrived, if one did.
#[must_use]
pub(crate) fn signal_raised() -> Option<i32> {
    let signal = SIGNAL.load(Ordering::SeqCst);
    (signal != 0).then_some(signal)
}

/// One case a child is told to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChildTarget {
    Case(String),
    Reading(String),
}

impl ChildTarget {
    pub(crate) fn argument(&self) -> String {
        match self {
            Self::Case(slug) => format!("writer:{slug}"),
            Self::Reading(name) => format!("reading:{name}"),
        }
    }

    fn parse(text: &str) -> Result<Self> {
        if let Some(slug) = text.strip_prefix("writer:") {
            return Ok(Self::Case(slug.to_string()));
        }
        if let Some(name) = text.strip_prefix("reading:") {
            return Ok(Self::Reading(name.to_string()));
        }
        bail!("the case must be `writer:<slug>` or `reading:<name>`, not {text:?}")
    }

    /// The case's own name, for a verdict's prose.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Case(slug) | Self::Reading(slug) => slug,
        }
    }
}

/// The hidden mode's arguments: a protocol with an arbitrary binary.
#[derive(Clone, Debug)]
pub struct ChildInvocation {
    pub target: ChildTarget,
    pub engine: EngineKind,
    pub root: PathBuf,
    pub transcript: PathBuf,
}

impl ChildInvocation {
    /// Parse the hidden mode's arguments, or answer `None` when this is an ordinary invocation.
    pub fn parse(args: &[String]) -> Result<Option<Self>> {
        let mut args = args.iter();
        if args.next().map(String::as_str) != Some("--internal-run") {
            return Ok(None);
        }
        let (mut target, mut engine, mut root, mut transcript) = (None, None, None, None);
        while let Some(arg) = args.next() {
            let mut value = || {
                args.next()
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("{arg} needs a value"))
            };
            match arg.as_str() {
                "--case" => target = Some(ChildTarget::parse(&value()?)?),
                "--engine" => {
                    let name = value()?;
                    engine = Some(
                        EngineKind::from_name(&name)
                            .ok_or_else(|| anyhow::anyhow!("unknown child engine {name:?}"))?,
                    );
                }
                "--root" => root = Some(PathBuf::from(value()?)),
                "--transcript" => transcript = Some(PathBuf::from(value()?)),
                other => bail!("unknown argument in the child mode: {other}"),
            }
        }
        Ok(Some(Self {
            target: target.context("the child mode needs --case")?,
            engine: engine.context("the child mode needs --engine")?,
            root: root.context("the child mode needs --root")?,
            transcript: transcript.context("the child mode needs --transcript")?,
        }))
    }
}

/// What the poller sampled, and what it decided.
#[derive(Default)]
pub(crate) struct Sampled {
    pub(crate) child_peak: u64,
    pub(crate) descendants_peak: u64,
    /// Whether the poller killed the case for outliving its budget. Every other kill is named by
    /// [`bounds_tripped`], which reads the state the case left behind and applies the stated order.
    pub(crate) timed_out: bool,
}

/// The budget of one case kind: a reading's two bounded engine calls and a writer's engine budget,
/// probe and materialisation are different work, so they get different stated numbers.
pub(crate) fn timeout_for(bounds: &Bounds, target: &ChildTarget) -> Duration {
    match target {
        ChildTarget::Case(_) => bounds.writer_timeout,
        ChildTarget::Reading(_) => bounds.reading_timeout,
    }
}

/// Poll the child until it exits, a bound trips, its budget is spent or a signal arrives.
pub(crate) fn poll(pid: i32, root: &Path, bounds: &Bounds, timeout: Duration) -> Sampled {
    let started = Instant::now();
    let mut sampled = Sampled::default();
    loop {
        if signal_raised().is_some() {
            // The child may have finished just as the signal arrived; its descendants are then
            // reparented and unidentifiable, and a sampled pid could already be reused.
            return if exited(pid) {
                sampled
            } else {
                torn_down(pid, sampled)
            };
        }
        if exited(pid) {
            return sampled;
        }
        if started.elapsed() > timeout {
            sampled.timed_out = true;
            return torn_down(pid, sampled);
        }
        if let Some(peak) = read_footprint(pid) {
            sampled.child_peak = sampled.child_peak.max(peak);
        }
        let descendants = tree(pid);
        // Each descendant's footprint is a lifetime maximum, so the tree's share is the sum of the
        // maxima seen this sample; the verdict takes the largest sum over samples. A descendant
        // that exits between samples contributes nothing, which is why this half is a lower bound.
        let tree_peak: u64 = descendants
            .iter()
            .filter_map(|descendant| read_footprint(*descendant))
            .sum();
        sampled.descendants_peak = sampled.descendants_peak.max(tree_peak);
        let usage = inspect(root, bounds);
        if usage.bytes > bounds.dir_bytes || usage.entries > bounds.dir_entries {
            return torn_down(pid, sampled);
        }
        let memory = sampled.child_peak + sampled.descendants_peak;
        if memory > bounds.memory_bytes {
            return torn_down(pid, sampled);
        }
        if wait_for_exit(pid, bounds.poll) {
            return sampled;
        }
    }
}

/// Kill the child's tree — descendants before the child — and hand the sample back, so every
/// teardown site is one expression and the order cannot drift between them. The walk happens here,
/// not from a pid sampled a poll interval ago: a recorded pid may have been reaped and reused.
fn torn_down(pid: i32, sampled: Sampled) -> Sampled {
    kill_tree(pid);
    sampled
}

/// Wait one poll interval, answering early when the child exits: a case that finishes in
/// milliseconds must not pay the whole interval before its verdict is decided.
fn wait_for_exit(pid: i32, interval: Duration) -> bool {
    let deadline = Instant::now() + interval;
    loop {
        if exited(pid) {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

/// Reap the child, killing its tree again if it does not die promptly. The poller already killed the
/// tree when a bound, a timeout or a signal tore it down; the pids it recorded may have been reaped
/// and reused by now, so this walks the tree afresh rather than signalling a recorded pid.
pub(crate) fn wait(child: &mut Child, pid: i32) -> Option<ExitStatus> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            // A signal the handler recorded may interrupt the wait; the signal path is checked by
            // the caller, and the child is still there to be reaped.
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
        if started.elapsed() > REAP_GRACE {
            kill_tree(pid);
        }
        if started.elapsed() > REAP_GRACE * 6 {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether `pid` has exited, without reaping it: the lifetime peak must be read first.
#[cfg(unix)]
fn exited(pid: i32) -> bool {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a valid, zeroed `siginfo_t`; the flags ask for an exited child without
    // reaping it.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    result == 0 && unsafe { info.si_pid() } != 0
}

#[cfg(not(unix))]
fn exited(_pid: i32) -> bool {
    false
}

/// Kill the child's whole process tree, descendants before the child.
fn kill_tree(pid: i32) {
    #[cfg(unix)]
    {
        // Enumerate while the descendants are still its descendants: killing the child first would
        // reparent them and make them unfindable. A second pass catches one that forked between the
        // walk and the kill; a double-forked descendant still escapes, and that is a stated hole.
        let descendants = tree(pid);
        for descendant in &descendants {
            unsafe {
                libc::kill(*descendant, libc::SIGKILL);
            }
        }
        for descendant in tree(pid) {
            unsafe {
                libc::kill(descendant, libc::SIGKILL);
            }
        }
        // A case may spawn a descendant in a process group of its own, which a group kill of the
        // child misses; the walk above is what reaches it. The group kill is a fallback for anything
        // the walk missed inside the child's own group.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
            libc::kill(pid, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

/// The descendants of `pid`, recursively: a grandchild is what a direct-children walk would leave
/// behind when its parent is killed.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn tree(pid: i32) -> Vec<i32> {
    let mut seen = Vec::new();
    let mut stack = descendants(pid);
    while let Some(descendant) = stack.pop() {
        if seen.contains(&descendant) {
            continue;
        }
        seen.push(descendant);
        stack.extend(descendants(descendant));
    }
    seen
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn tree(_pid: i32) -> Vec<i32> {
    Vec::new()
}

/// The direct children of `pid`.
#[cfg(target_os = "macos")]
fn descendants(pid: i32) -> Vec<i32> {
    let mut buffer = vec![0i32; 64];
    loop {
        // SAFETY: the buffer is valid for its stated length; the call writes pids into it and
        // answers their count.
        let count = unsafe {
            libc::proc_listchildpids(
                pid,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * std::mem::size_of::<i32>()) as libc::c_int,
            )
        };
        if count <= 0 {
            return Vec::new();
        }
        let count = count as usize;
        if count < buffer.len() {
            buffer.truncate(count);
            return buffer;
        }
        buffer.resize(buffer.len() * 2, 0);
    }
}

/// The direct children of `pid`, from `/proc`.
#[cfg(target_os = "linux")]
fn descendants(pid: i32) -> Vec<i32> {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .map(|text| {
            text.split_whitespace()
                .filter_map(|pid| pid.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The direct children of `pid` where the platform cannot enumerate them.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn descendants(_pid: i32) -> Vec<i32> {
    Vec::new()
}

/// The physical footprint of `pid` as a lifetime maximum, where the platform reports it.
#[cfg(target_os = "macos")]
pub(crate) fn read_footprint(pid: i32) -> Option<u64> {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a valid, zeroed `rusage_info_v4`; the flavor is the one whose layout it has.
    let result = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V4,
            &mut info as *mut libc::rusage_info_v4 as *mut libc::rusage_info_t,
        )
    };
    (result == 0).then_some(info.ri_lifetime_max_phys_footprint)
}

/// The physical footprint where the platform cannot report it: the bound is not enforced.
#[cfg(not(target_os = "macos"))]
pub(crate) fn read_footprint(_pid: i32) -> Option<u64> {
    None
}

/// What a case's directory holds. A symlink is neither followed nor charged: a link to a tree
/// outside the root must not trip a bound the case never reached.
struct Usage {
    bytes: u64,
    entries: usize,
    largest: u64,
    /// Whether the walk reached the cap before the directory was exhausted; the entry count and the
    /// byte total are then lower bounds.
    complete: bool,
}

/// Measure a case's directory. The walk is capped so a flood cannot make the parent walk without
/// limit; it stops early when the file-size bound — the first in the stated order — is settled, or
/// at the cap. The directory-size and entry-count answers are compared in the stated order, so a
/// case over both is named by the directory-size bound rather than by whichever entry came first.
fn inspect(root: &Path, bounds: &Bounds) -> Usage {
    let cap = walk_limit(bounds.dir_entries);
    let mut usage = Usage {
        bytes: 0,
        entries: 0,
        largest: 0,
        complete: true,
    };
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(listing) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in listing.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            usage.entries += 1;
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if let Ok(metadata) = entry.metadata() {
                usage.bytes += allocated(&metadata);
                usage.largest = usage.largest.max(metadata.len());
            }
            if usage.largest >= bounds.file_bytes || usage.entries >= cap {
                // The walk stopped before the directory was exhausted, so the entry count and the
                // byte total are lower bounds, not the real ones.
                usage.complete = false;
                return usage;
            }
        }
    }
    usage
}

/// The entries a walk may count before it stops: sixteen times the entry bound plus a margin, so the
/// entry bound is always decided before the cap and a flood cannot make the parent walk without
/// limit.
fn walk_limit(dir_entries: usize) -> usize {
    dir_entries.saturating_mul(16).saturating_add(256)
}

/// The bytes charged for one file: allocated bytes, so a sparse file is not charged its apparent
/// size. The per-file apparent-size limit is what catches those.
#[cfg(unix)]
fn allocated(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn allocated(metadata: &std::fs::Metadata) -> u64 {
    metadata.len()
}

/// The first bound the case's directory and tree have tripped, in the stated order: the file-size
/// bound, then the directory-size bound, then the entry-count bound, then the memory bound.
pub(crate) fn bounds_tripped(
    root: &Path,
    bounds: &Bounds,
    child_peak: u64,
    descendants_peak: u64,
) -> Option<String> {
    let usage = inspect(root, bounds);
    if usage.largest >= bounds.file_bytes {
        return Some(format!(
            "a file in the case's directory is {} bytes, at the {}-byte per-file bound",
            usage.largest, bounds.file_bytes
        ));
    }
    if usage.bytes > bounds.dir_bytes {
        return Some(format!(
            "the case's directory holds {} allocated bytes, over the {}-byte directory bound",
            usage.bytes, bounds.dir_bytes
        ));
    }
    if usage.entries > bounds.dir_entries {
        let counted = if usage.complete {
            usage.entries.to_string()
        } else {
            format!("at least {}", usage.entries)
        };
        return Some(format!(
            "the case's directory holds {counted} entries, over the {}-entry bound",
            bounds.dir_entries
        ));
    }
    let memory = child_peak + descendants_peak;
    if memory > bounds.memory_bytes {
        return Some(format!(
            "the case's process tree peaked at {memory} bytes of physical footprint (child \
             {child_peak}, descendants {descendants_peak}), over the {}-byte memory bound",
            bounds.memory_bytes
        ));
    }
    None
}

/// Keep only the last [`STREAM_TAIL_BYTES`] of a child's stream, on a thread that drains it.
pub(crate) fn tail(mut stream: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut tail: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    tail.extend_from_slice(&buffer[..read]);
                    if tail.len() > STREAM_TAIL_BYTES {
                        let excess = tail.len() - STREAM_TAIL_BYTES;
                        tail.drain(..excess);
                    }
                }
            }
        }
        let _ = sender.send(tail);
    });
    receiver
}

/// The tail a drain kept, or nothing when a descendant still holds the pipe.
pub(crate) fn drain(receiver: Option<mpsc::Receiver<Vec<u8>>>) -> String {
    receiver
        .and_then(|receiver| receiver.recv_timeout(DRAIN_GRACE).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// The name every harness root carries: the owning process, its start time before the nonce, and
/// the nonce. The start time is what lets a leftover root's owner be identified after its identifier
/// is reused; the nonce is the last segment, so the transcript normaliser finds it where it looks.
pub(crate) fn root_name(nonce: u64) -> String {
    let (seconds, microseconds) = owner_start();
    format!(
        "{}{}-{seconds:x}-{microseconds:x}-{nonce:016x}",
        ROOT_PREFIX,
        std::process::id()
    )
}

/// The owning process's start time, as the platform records it. `(0, 0)` where it cannot be read;
/// the marker then records that no start time is known, and [`owner_alive`] falls back to the
/// identifier rather than reading a live owner as dead.
fn owner_start() -> (u64, u64) {
    match process_start(std::process::id()) {
        Probe::At(seconds, microseconds) => (seconds, microseconds),
        Probe::Gone | Probe::Unknown => (0, 0),
    }
}

/// What the platform says about a process's start time.
enum Probe {
    At(u64, u64),
    Gone,
    Unknown,
}

/// The start time of `pid`, where the platform can report it.
#[cfg(target_os = "macos")]
fn process_start(pid: u32) -> Probe {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a valid, zeroed `proc_bsdinfo`; the flavor is the one whose layout it has.
    let result = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut libc::proc_bsdinfo as *mut libc::c_void,
            std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int,
        )
    };
    if result as usize == std::mem::size_of::<libc::proc_bsdinfo>() {
        return Probe::At(info.pbi_start_tvsec, info.pbi_start_tvusec);
    }
    // SAFETY: reading this thread's errno is safe.
    let errno = std::io::Error::last_os_error().raw_os_error();
    if errno == Some(libc::ESRCH) {
        Probe::Gone
    } else {
        Probe::Unknown
    }
}

/// The start time of `pid`, from `/proc`.
#[cfg(target_os = "linux")]
fn process_start(pid: u32) -> Probe {
    let path = format!("/proc/{pid}/stat");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return if Path::new(&path).exists() {
            Probe::Unknown
        } else {
            Probe::Gone
        };
    };
    // The command name may hold spaces and parentheses; the fields after the last `)` are the
    // numbered ones, and the start time is field 22, the twentieth after the state.
    let Some(rest) = text.rsplit_once(')').map(|(_, rest)| rest) else {
        return Probe::Unknown;
    };
    rest.split_whitespace()
        .nth(19)
        .and_then(|ticks| ticks.parse().ok())
        .map_or(Probe::Unknown, |ticks| Probe::At(ticks, 0))
}

/// A process's start time where the platform cannot report it: a gone process is gone, and one that
/// exists cannot be told apart from a reused identifier, so it counts as alive.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_start(pid: u32) -> Probe {
    #[cfg(unix)]
    {
        // No start time is available, so existence is the only evidence: a gone process is gone, and
        // one that exists cannot be told apart from a reused identifier, so it counts as alive.
        if unsafe { libc::kill(pid as i32, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return Probe::Gone;
        }
    }
    let _ = pid;
    Probe::Unknown
}

/// A root's marker text: the owning process's identifier and start time.
fn marker_text() -> String {
    let (seconds, microseconds) = owner_start();
    format!("{} {seconds} {microseconds}\n", std::process::id())
}

/// A root's claim: its marker file, locked exclusively for as long as the guard lives.
///
/// The kernel drops the lock when the process exits, however it exits, so a sweep proves an owner
/// gone instead of inferring it from an identifier that may have been reused. A root with no claim
/// is not proof of death — an older harness never took one, and the moment between creating a root
/// and claiming it is real — so the sweep still asks the marker's owner.
#[must_use = "dropping the claim releases the lock, which is the root's only proof of life"]
pub struct Claim {
    _marker: std::fs::File,
}

impl Claim {
    /// Write `root`'s marker and lock it. The lock is taken before the marker is rewritten, so a
    /// refused lock leaves the previous owner's recorded identity intact.
    pub fn take(root: &Path) -> std::io::Result<Self> {
        let mut marker = std::fs::OpenOptions::new()
            .create(true)
            // Truncated only after the lock is taken, so a refused lock leaves the marker honest.
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(ROOT_MARKER))?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd as _;
            // SAFETY: the descriptor is open for the guard's lifetime; `flock` only takes a lock.
            if unsafe { libc::flock(marker.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        marker.set_len(0)?;
        marker.write_all(marker_text().as_bytes())?;
        marker.flush()?;
        Ok(Self { _marker: marker })
    }
}

/// What a root's claim says about its owner.
enum Claimed {
    /// A live process holds it: the root belongs to a run that is still going.
    Held,
    /// Nobody holds it, so its owner is gone or never claimed it. Not yet proof of abandonment.
    Free,
    /// It cannot be tested: the safe answer is to leave the root for a later run.
    Unknown,
}

/// Whether a live process holds `root`'s claim.
fn claimed(root: &Path) -> Claimed {
    let Ok(marker) = std::fs::File::open(root.join(ROOT_MARKER)) else {
        return Claimed::Unknown;
    };
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd as _;
        // SAFETY: the descriptor is open for the duration of the calls. A successful lock is
        // released at once: holding it would hide a root this sweep leaves in place from a later
        // sweep.
        unsafe {
            if libc::flock(marker.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 {
                libc::flock(marker.as_raw_fd(), libc::LOCK_UN);
                return Claimed::Free;
            }
        }
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) {
            Claimed::Held
        } else {
            Claimed::Unknown
        }
    }
    #[cfg(not(unix))]
    {
        // No advisory locking here, so no claim can be tested: every leftover root is left for a
        // later run. That is litter, never a lost verdict.
        let _ = marker;
        Claimed::Unknown
    }
}

/// Reclaim leftover roots under the OS temporary directory: see [`sweep_in`].
pub fn sweep(own_root: &Path, bounds: &Bounds) {
    sweep_in(&std::env::temp_dir(), Some(own_root), bounds);
}

/// Reclaim leftover roots under `directory`: only those carrying the harness's own marker, which no
/// live process claims, whose recorded owner is no longer alive, and which are older than the
/// grace. `own_root` is the sweeping run's own root, never considered; `None` considers every root
/// under `directory`. A claim that cannot be tested counts as alive, and so does a probe that cannot
/// decide. Each reclamation is reported on standard error, so a root that disappears leaves evidence
/// of what took it, and every reclaimed path is returned. The sweep never runs in a child, which
/// would sweep its siblings' roots.
pub fn sweep_in(directory: &Path, own_root: Option<&Path>, bounds: &Bounds) -> Vec<PathBuf> {
    let mut reclaimed = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return reclaimed;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if own_root == Some(path.as_path()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(ROOT_PREFIX) {
            continue;
        }
        // A root is a real directory this harness created: a symlink is not followed, because
        // removing it would remove something outside the temporary area.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(marker) = std::fs::read_to_string(path.join(ROOT_MARKER)) else {
            continue;
        };
        let Some((pid, seconds, microseconds)) = parse_marker(&marker) else {
            continue;
        };
        // The claim is the proof of life: a live owner holds it, and the kernel drops it when the
        // owner dies, however it dies.
        match claimed(&path) {
            Claimed::Held | Claimed::Unknown => continue,
            Claimed::Free => {}
        }
        // A root that was never claimed — an older harness's, or one created in the moment before
        // its claim was taken — is decided by the probe instead.
        if owner_alive(pid, seconds, microseconds) {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let age = std::time::SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default();
        if age <= bounds.grace {
            continue;
        }
        // The reclamation is reported only once the root is gone: a removal that failed already
        // named itself, and must not also read as reclaimed.
        if discard(&path) {
            crate::modes::err(&format!(
                "gate: reclaimed the leftover root {} (owner {pid} is gone)",
                path.display()
            ));
            reclaimed.push(path);
        }
    }
    reclaimed
}

/// Remove a root, releasing its claim first: a platform where an open file blocks removal would
/// otherwise leak the marker inside it. The one home of that order.
pub(crate) fn remove_root(root: &Path, claim: Option<Claim>) {
    drop(claim);
    discard(root);
}

/// Remove a directory tree, reporting a failure rather than swallowing it: a root that cannot be
/// removed is a leak, retried by a later run's sweep once the grace has passed. Returns whether
/// this call removed it: a tree another sweep already took is gone, but not this call's to report.
pub(crate) fn discard(path: &Path) -> bool {
    match std::fs::remove_dir_all(path) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            // A concurrent sweep can empty the tree from under this one, which the platform then
            // reports as a failure. The tree is gone either way; only a survivor is a leak.
            if path.exists() {
                crate::modes::err(&format!("gate: cannot reclaim {}: {e}", path.display()));
            }
            false
        }
    }
}

fn parse_marker(text: &str) -> Option<(u32, u64, u64)> {
    let mut fields = text.split_whitespace();
    Some((
        fields.next()?.parse().ok()?,
        fields.next()?.parse().ok()?,
        fields.next()?.parse().ok()?,
    ))
}

/// Whether the marker's owner is still the process it says it is. A marker that recorded no start
/// time leaves the identifier as the only evidence, and a live identifier is left alone.
fn owner_alive(pid: u32, seconds: u64, microseconds: u64) -> bool {
    match process_start(pid) {
        // The marker recorded no start time: the identifier is the only evidence.
        Probe::At(..) if (seconds, microseconds) == (0, 0) => true,
        // An identifier reused by an unrelated process has a different start time.
        Probe::At(live_seconds, live_microseconds) => {
            (live_seconds, live_microseconds) == (seconds, microseconds)
        }
        // A process that cannot be identified but exists is left alone: a permission error on the
        // probe must never delete a live run's work.
        Probe::Unknown => true,
        Probe::Gone => false,
    }
}

/// The identity a helper engine answers. The helper binary and the parent's stand-in must agree, so
/// the literal has one home.
#[must_use]
pub fn helper_identity() -> EngineIdentity {
    EngineIdentity {
        engine: "helper".to_string(),
        build: "helper".to_string(),
        runtime: None,
        components: std::collections::BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A claim is the only evidence the sweep treats as proof, so each of its three answers is
    /// pinned: free means abandoned, held means alive, and unknown means leave it alone.
    #[test]
    #[cfg(unix)]
    fn a_claim_reads_free_held_or_untestable() {
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let root = scratch.path().join("document-gate-claim");
        std::fs::create_dir(&root).expect("a root");
        let claim = Claim::take(&root).expect("a claim");
        assert!(
            matches!(claimed(&root), Claimed::Held),
            "a held claim is held"
        );
        drop(claim);
        assert!(
            matches!(claimed(&root), Claimed::Free),
            "the kernel releases the claim with the guard"
        );
        std::fs::remove_file(root.join(ROOT_MARKER)).expect("remove the marker");
        assert!(
            matches!(claimed(&root), Claimed::Unknown),
            "a claim that cannot be opened cannot be tested"
        );
    }
}
