//! The outside reference: the LibreOffice installation this machine has, and one conversion per
//! document.
//!
//! The reference is found in the system, the ordinary way — never unpacked into a temporary
//! location, never run out of a mounted image — and the run records the version and build it
//! actually found. A run against another build refuses rather than producing a record that means
//! nothing.
//!
//! Every conversion is bounded: its processes and every process they start are killed when the
//! deadline passes and reaped afterwards, because killing only the launcher leaves children holding
//! the profile. Its pipes are read with a deadline of their own, so a process that left the child's
//! group and still holds one cannot hold the run past it. The profile is one per run and is removed
//! when the run ends, so two runs cannot collide. Success is judged by the produced document, not
//! by an exit code: a conversion that exits zero and produces nothing is a failure.
//!
//! The record is made on macOS: the font directories it supplies are that machine's own, and the
//! reaping below relies on a platform whose `kill` reports a group whose only member is an unreaped
//! zombie as gone (see `group_alive`).
//!
//! Nothing here downloads or installs anything, and the shipped crate neither needs nor knows about
//! it.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::{Format, LOCALE};

/// How long one conversion may take: the launcher, and every process it starts.
pub const CONVERSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// How long the launcher may take to answer `--version`.
const VERSION_TIMEOUT: Duration = Duration::from_secs(30);

/// How often a running child is asked whether it has exited.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// How long the process group is given to end after it has been asked to.
const REAP_GRACE: Duration = Duration::from_secs(5);

/// How much of a child's output is kept, in bytes.
const TAIL_BYTES: usize = 8 * 1024;

/// How much of a child's output an error sentence quotes, in bytes.
const QUOTED_BYTES: usize = 2000;

/// The launchers a macOS installation has: the one inside the bundle, then the bundle itself, whose
/// launcher is looked for inside it.
const BUNDLE_LAUNCHERS: &[&str] = &[
    "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    "/Applications/LibreOffice.app",
];

/// The launchers a Unix installation has at a conventional place.
const UNIX_LAUNCHERS: &[&str] = &[
    "/usr/lib/libreoffice/program/soffice",
    "/usr/bin/soffice",
    "/opt/libreoffice/program/soffice",
];

/// The names a launcher has on `PATH`.
const PATH_NAMES: &[&str] = &["soffice", "libreoffice"];

/// The arguments every conversion is given, in order. They state what the record's identity states:
/// a headless run with no restore, no lock file, no default document and no first-start wizard, and
/// a user profile of its own.
pub const ARGUMENTS: &[&str] = &[
    "--headless",
    "--norestore",
    "--nolockcheck",
    "--nodefault",
    "--nofirststartwizard",
];

/// The reference this machine has.
pub struct Reference {
    /// The program's name.
    pub program: String,
    /// Its version, as it reports it.
    pub version: String,
    /// Its build, as it reports it.
    pub build: String,
    /// Where it is installed.
    pub installation: PathBuf,
    /// The launcher the run drives.
    pub launcher: PathBuf,
}

/// Find the installation this machine has, and ask it for its version.
///
/// The probe is the one invocation that runs before a run's profile exists: it is given the
/// operating system's temporary directory as its working directory and `TMPDIR`, so it writes
/// nothing into this package, but the per-run profile isolation covers conversions, not this one
/// question.
///
/// # Errors
///
/// No installation is found, or it answers with no version.
pub fn find() -> Result<Reference, String> {
    let launcher = locate()?;
    let installation = installation_of(&launcher);
    let mut command = Command::new(&launcher);
    command.arg("--version");
    command.current_dir(std::env::temp_dir());
    command.env("TMPDIR", std::env::temp_dir());
    let finished = run(command, VERSION_TIMEOUT)?;
    if finished.timed_out {
        return Err(format!(
            "{} did not answer --version within {} seconds",
            launcher.display(),
            VERSION_TIMEOUT.as_secs()
        ));
    }
    if finished.status.is_some_and(|status| !status.success()) {
        return Err(format!(
            "{} answered --version and then failed: {}",
            launcher.display(),
            finished.ending()
        ));
    }
    let (version, build) = parse_version(&finished.output).ok_or_else(|| {
        format!(
            "{} answers --version with no version: {}",
            launcher.display(),
            quoted(&finished.output)
        )
    })?;
    Ok(Reference {
        program: "LibreOffice".to_string(),
        version,
        build,
        installation,
        launcher,
    })
}

/// The launcher this machine has, looked for the ordinary way.
fn locate() -> Result<PathBuf, String> {
    let mut looked = Vec::new();
    for candidate in BUNDLE_LAUNCHERS {
        looked.push((*candidate).to_string());
        if let Some(launcher) = launcher_at(Path::new(candidate)) {
            return Ok(launcher);
        }
    }
    for name in PATH_NAMES {
        looked.push(format!("{name} on PATH"));
        if let Some(launcher) = on_path(name) {
            return Ok(launcher);
        }
    }
    for candidate in UNIX_LAUNCHERS {
        looked.push((*candidate).to_string());
        if let Some(launcher) = launcher_at(Path::new(candidate)) {
            return Ok(launcher);
        }
    }
    Err(format!(
        "the outside reference is not found: there is no launcher at any of {}",
        looked.join(", ")
    ))
}

/// The launcher at `path`, which is either a launcher itself or an installation holding one.
fn launcher_at(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    ["Contents/MacOS/soffice", "program/soffice"]
        .into_iter()
        .map(|inside| path.join(inside))
        .find(|candidate| candidate.is_file())
}

/// The first `name` on `PATH`, looked for without a shell.
fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// Where the launcher's installation is: the bundle it sits in, else the prefix two levels above
/// `program/soffice`, else the launcher's own directory.
fn installation_of(launcher: &Path) -> PathBuf {
    if let Some(bundle) = launcher.ancestors().find(|ancestor| {
        ancestor
            .extension()
            .is_some_and(|extension| extension == "app")
    }) {
        return bundle.to_path_buf();
    }
    match launcher.parent() {
        Some(program) if program.ends_with("program") => match program.parent() {
            Some(prefix) => prefix.to_path_buf(),
            None => program.to_path_buf(),
        },
        Some(directory) => directory.to_path_buf(),
        None => PathBuf::new(),
    }
}

/// The version and build a `--version` answer states.
fn parse_version(output: &str) -> Option<(String, String)> {
    let line = output
        .lines()
        .find(|line| line.trim_start().starts_with("LibreOffice "))?;
    let mut words = line.split_whitespace();
    if words.next()? != "LibreOffice" {
        return None;
    }
    let version = words.next()?.to_string();
    Some((version, words.next().unwrap_or_default().to_string()))
}

/// One run's converter: a profile of its own, the font configuration it is given, and the deadline
/// every conversion is bounded by.
pub struct Converter {
    /// The launcher.
    launcher: PathBuf,
    /// The run's profile, which the reference owns and the run removes.
    profile: PathBuf,
    /// The font configuration's path.
    fonts: PathBuf,
    /// The cache directory the reference is told to use, so it writes inside the run's area.
    cache: PathBuf,
    /// The temporary directory the reference is told to use, for the same reason.
    tmp: PathBuf,
}

impl Converter {
    /// Prepare a converter under `root`: the profile with the pinned locale already written into it,
    /// and the font configuration the reference is given.
    ///
    /// The locale is pinned before the reference ever runs, because it decides a workbook's default
    /// paper: a profile written after a first run would be a profile the reference has already
    /// decided for itself.
    ///
    /// # Errors
    ///
    /// The profile or the configuration cannot be written.
    pub fn new(reference: &Reference, root: &Path, fonts: &str) -> Result<Self, String> {
        directory(root)?;
        let profile = root.join("profile");
        let user = profile.join("user");
        directory(&user)?;
        write(&user.join("registrymodifications.xcu"), &locale_pin())?;
        let fonts_path = root.join("fonts.conf");
        write(&fonts_path, fonts)?;
        let cache = root.join("cache");
        directory(&cache)?;
        let tmp = root.join("tmp");
        directory(&tmp)?;
        Ok(Self {
            launcher: reference.launcher.clone(),
            profile,
            fonts: fonts_path,
            cache,
            tmp,
        })
    }

    /// Convert one document and answer with the produced PDF's bytes.
    ///
    /// The produced file is the input's stem plus `.pdf`, so the directory is required to hold no
    /// such file beforehand: an older document with the same name must never be mistaken for this
    /// run's output.
    ///
    /// The input's digest is taken before and after the conversion, and a conversion that changed
    /// the document it was given is refused: the record states that the reference never modifies its
    /// input, so that is checked rather than assumed.
    ///
    /// # Errors
    ///
    /// The input cannot be read, the conversion timed out, failed or produced nothing, or the
    /// reference modified the document it was given.
    pub fn convert(&self, input: &Path, out_dir: &Path) -> Result<Vec<u8>, String> {
        directory(out_dir)?;
        let format = format_of(input)?;
        let stem = input.file_stem().ok_or_else(|| {
            format!(
                "{} states no file name, so no produced document can be named after it",
                input.display()
            )
        })?;
        let produced = out_dir.join(format!("{}.pdf", stem.to_string_lossy()));
        if produced.exists() {
            return Err(format!(
                "{} already holds a document this run did not produce; a conversion needs a directory of its own",
                produced.display()
            ));
        }
        let given = std::fs::read(input)
            .map_err(|error| format!("cannot read {}: {error}", input.display()))?;
        let given_digest = crate::canonical::digest(&given);
        let mut command = Command::new(&self.launcher);
        command.args(ARGUMENTS);
        command.arg(format!("-env:UserInstallation={}", file_url(&self.profile)));
        command
            .arg("--convert-to")
            .arg(format!("pdf:{}", format.filter()));
        command.arg("--outdir").arg(out_dir).arg(input);
        command.env("FONTCONFIG_FILE", &self.fonts);
        command.env("XDG_CACHE_HOME", &self.cache);
        command.env("TMPDIR", &self.tmp);
        // The reference never inherits this package as its working directory.
        command.current_dir(&self.tmp);
        let finished = run(command, CONVERSION_TIMEOUT)?;
        let after = std::fs::read(input)
            .map_err(|error| format!("cannot read {} again: {error}", input.display()))?;
        if crate::canonical::digest(&after) != given_digest {
            return Err(format!(
                "the reference modified the document it was given at {}: a conversion must leave its input as it found it",
                input.display()
            ));
        }
        let bytes = std::fs::read(&produced)
            .ok()
            .filter(|bytes| !bytes.is_empty());
        if finished.timed_out {
            return Err(format!(
                "the reference did not finish within {} seconds and was killed, so the document at {} is not accepted: {}",
                CONVERSION_TIMEOUT.as_secs(),
                produced.display(),
                quoted(&finished.output)
            ));
        }
        match bytes {
            Some(bytes) => Ok(bytes),
            None => Err(format!(
                "the reference produced no document at {}: it {}, and wrote {}",
                produced.display(),
                finished.ending(),
                quoted(&finished.output)
            )),
        }
    }
}

/// `path` as a `file://` URL, percent-encoded.
///
/// A path is not a URL: a space in it or a `#` would be read as a URL's space or its fragment, and
/// the reference would be handed a profile that is not the run's. ASCII alphanumerics and `-._~/`
/// are kept as they are and every other byte is encoded as `%XX`, so the URL is the same bytes the
/// path holds, whatever the temporary area's name contains.
fn file_url(path: &Path) -> String {
    #[cfg(unix)]
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str());
    #[cfg(not(unix))]
    let lossy = path.to_string_lossy();
    #[cfg(not(unix))]
    let bytes = lossy.as_bytes();
    let mut url = String::with_capacity("file://".len() + bytes.len());
    url.push_str("file://");
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            url.push(char::from(byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

/// The family a document's extension states.
fn format_of(input: &Path) -> Result<Format, String> {
    let extension = input
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    Format::parse(&extension).ok_or_else(|| {
        format!(
            "{}: no family the record covers has the extension {extension:?}",
            input.display()
        )
    })
}

/// The profile's locale pin, written before the reference's first run.
fn locale_pin() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <oor:items xmlns:oor=\"http://openoffice.org/2001/registry\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n\
         <item oor:path=\"/org.openoffice.Setup/L10N\"><prop oor:name=\"ooSetupSystemLocale\" oor:op=\"fuse\"><value>{LOCALE}</value></prop></item>\n\
         </oor:items>\n"
    )
}

/// Create `path` and every directory above it.
fn directory(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))
}

/// Write one file of the run's area.
fn write(path: &Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// What one bounded child run left behind.
struct Finished {
    /// How it exited, when it exited on its own.
    status: Option<std::process::ExitStatus>,
    /// The tail of what it wrote, at most [`TAIL_BYTES`] long.
    output: String,
    /// Whether the deadline passed and it had to be killed.
    timed_out: bool,
}

impl Finished {
    /// How it ended, as an error sentence states it.
    fn ending(&self) -> String {
        match self.status {
            Some(status) => format!("exited with {status}"),
            None => "did not exit".to_string(),
        }
    }
}

/// Run one child to its end or to its deadline, and answer with what it left behind.
fn run(mut command: Command, timeout: Duration) -> Result<Finished, String> {
    let program = command.get_program().to_string_lossy().into_owned();
    // A child that reads standard input must not read ours: it would block on it, or consume what
    // the caller meant for something else, past every bound stated here.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start {program}: {error}"))?;
    let stdout = child.stdout.take().map(read_tail);
    let stderr = child.stderr.take().map(read_tail);
    let pid = child.id();
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match exited(&mut child) {
            Ok(true) => {
                // The leader has exited but is still unreaped, so its process group id is still the
                // launcher's and `reap` cannot signal a later process that reused it.
                reap(pid);
                break child.wait().ok();
            }
            Ok(false) => {}
            Err(error) => {
                stop(&mut child);
                reap(pid);
                let _ = child.wait();
                return Err(error);
            }
        }
        if Instant::now() >= deadline {
            timed_out = true;
            stop(&mut child);
            reap(pid);
            break child.wait().ok();
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    Ok(Finished {
        status,
        output: collected(stdout, stderr),
        timed_out,
    })
}

/// Whether `child` has exited, without reaping it: the leader is left for [`Child::wait`], so the
/// process group it leads stays reserved while the group is being reaped.
///
/// `waitid`'s `WNOWAIT` answers without reaping, and it is POSIX: on every Unix the group is
/// signalled before the leader's pid can be reused. Elsewhere [`Child::try_wait`] reaps the leader,
/// so the group is signalled after the leader's pid is free; the operating system would have to
/// reuse that pid as a new group in the microseconds before the signal for the wrong process to be
/// signalled.
fn exited(child: &mut Child) -> Result<bool, String> {
    let pid = child.id();
    #[cfg(unix)]
    {
        // SAFETY: `waitid` only fills in the `siginfo_t` it is given, which is read only after it
        // has answered successfully; `WNOWAIT` leaves the leader unreaped.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let answered = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
            )
        };
        if answered != 0 {
            return Err(format!(
                "cannot wait for {pid}: {}",
                std::io::Error::last_os_error()
            ));
        }
        #[cfg(target_os = "macos")]
        let exited = info.si_pid != 0;
        // SAFETY: `waitid` answered, so the union holds the fields it states for `WEXITED`.
        #[cfg(not(target_os = "macos"))]
        let exited = unsafe { info.si_pid() != 0 };
        Ok(exited)
    }
    #[cfg(not(unix))]
    {
        child
            .try_wait()
            .map(|status| status.is_some())
            .map_err(|error| format!("cannot wait for {pid}: {error}"))
    }
}

/// Read a pipe to its end on a thread of its own, keeping its last [`TAIL_BYTES`], and answer with
/// the channel it sends them on when the pipe ends.
fn read_tail(mut pipe: impl std::io::Read + Send + 'static) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    kept.extend_from_slice(&chunk[..read]);
                    if kept.len() > TAIL_BYTES {
                        let excess = kept.len() - TAIL_BYTES;
                        kept.drain(..excess);
                    }
                }
            }
        }
        let _ = sender.send(kept);
    });
    receiver
}

/// Collect both readers, giving each [`REAP_GRACE`] to answer, and answer with the tail of what
/// they kept. A reader that does not answer in time kept nothing: a process that left the child's
/// group and still holds its pipe cannot be waited for, so its reader thread is left to the
/// operating system rather than joined.
fn collected(
    stdout: Option<std::sync::mpsc::Receiver<Vec<u8>>>,
    stderr: Option<std::sync::mpsc::Receiver<Vec<u8>>>,
) -> String {
    let mut kept = Vec::new();
    for reader in [stdout, stderr].into_iter().flatten() {
        if let Ok(bytes) = reader.recv_timeout(REAP_GRACE) {
            kept.extend_from_slice(&bytes);
        }
    }
    if kept.len() > TAIL_BYTES {
        let excess = kept.len() - TAIL_BYTES;
        kept.drain(..excess);
    }
    String::from_utf8_lossy(&kept).trim().to_string()
}

/// Give the child a process group of its own, so everything it starts can be stopped together.
#[cfg(unix)]
fn isolate(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;
    command.process_group(0);
}

/// Nothing to isolate: a process is not given a group of its own here.
#[cfg(not(unix))]
fn isolate(_command: &mut Command) {}

/// Stop the child, and every process it started.
fn stop(child: &mut Child) {
    #[cfg(unix)]
    signal_group(child.id(), libc::SIGKILL);
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

/// Send `signal` to the process group `pid` leads.
#[cfg(unix)]
fn signal_group(pid: u32, signal: libc::c_int) {
    if let Ok(pid) = libc::pid_t::try_from(pid) {
        // SAFETY: `kill` only reads the two numbers it is given; a negative pid addresses the group
        // the launcher leads and nothing else.
        unsafe { libc::kill(-pid, signal) };
    }
}

/// Whether the process group `pid` leads still has a live member.
///
/// On macOS a group whose only remaining member is an unreaped zombie answers `EPERM` rather than
/// `0`, so a zombie counts as no member here. The group id itself stays reserved while the leader
/// is unreaped, which is what lets [`reap`] be called before [`Child::wait`]. A platform that
/// answers `0` for such a group pays [`REAP_GRACE`] after every conversion; the record is made on
/// macOS, where it does not.
#[cfg(unix)]
fn group_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal zero is the existence check `kill` defines, and it touches no process.
    let answered = unsafe { libc::kill(-pid, 0) };
    answered == 0
}

/// Stop what the launcher left behind: a launcher that has exited leaves children holding the
/// profile, and the next run would find the profile busy.
fn reap(pid: u32) {
    #[cfg(unix)]
    {
        if !group_alive(pid) {
            return;
        }
        signal_group(pid, libc::SIGTERM);
        let deadline = Instant::now() + REAP_GRACE;
        while Instant::now() < deadline {
            if !group_alive(pid) {
                return;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        signal_group(pid, libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// At most [`QUOTED_BYTES`] of a child's output, for an error sentence.
fn quoted(output: &str) -> String {
    let text = output.trim();
    if text.is_empty() {
        return "no output".to_string();
    }
    let mut end = text.len().min(QUOTED_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("\"{}\"", &text[..end])
}

/// A probe document naming one face, so the run can tell whether the reference could see the
/// machine's font set at all. A minimal Word document: one paragraph, one run, one stated face.
///
/// It is built in memory and carries nothing but the three parts Word needs to open it: the content
/// types, the package relationship and the document body.
///
/// # Errors
///
/// The archive cannot be assembled.
pub fn probe_document(face: &str) -> Result<Vec<u8>, String> {
    use std::io::Write as _;

    let face = crate::escape(face);
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
         </Types>";
    let relationships = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
         </Relationships>";
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>\
         <w:p><w:r><w:rPr><w:rFonts w:ascii=\"{face}\" w:hAnsi=\"{face}\" w:cs=\"{face}\"/></w:rPr>\
         <w:t>The quick brown fox jumps over the lazy dog. 0123456789</w:t></w:r></w:p>\
         <w:sectPr/></w:body></w:document>"
    );
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", relationships),
        ("word/document.xml", document.as_str()),
    ] {
        writer
            .start_file(name, options)
            .map_err(|error| format!("cannot assemble the probe document's {name}: {error}"))?;
        writer
            .write_all(body.as_bytes())
            .map_err(|error| format!("cannot assemble the probe document's {name}: {error}"))?;
    }
    let cursor = writer
        .finish()
        .map_err(|error| format!("cannot assemble the probe document: {error}"))?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One part of an archive as text.
    fn part(archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> String {
        use std::io::Read as _;
        let mut file = archive.by_name(name).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn the_profile_and_the_conversion_are_not_among_the_fixed_arguments() {
        for argument in ARGUMENTS {
            assert!(
                !argument.starts_with("-env:") && !argument.starts_with("--convert-to"),
                "{argument} belongs to one run, not to every conversion"
            );
        }
    }

    #[test]
    fn the_probe_document_holds_the_three_parts_word_needs() {
        let bytes = probe_document("Helvetica").unwrap();
        assert!(!bytes.is_empty());
        let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut names: Vec<String> = archive.file_names().map(str::to_string).collect();
        names.sort();
        assert_eq!(
            names,
            ["[Content_Types].xml", "_rels/.rels", "word/document.xml"]
        );
    }

    #[test]
    fn the_probe_document_states_the_face_it_is_given() {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(probe_document("Helvetica").unwrap()))
                .unwrap();
        let document = part(&mut archive, "word/document.xml");
        assert!(document.contains(r#"w:ascii="Helvetica""#), "{document}");
        assert!(document.contains(r#"w:hAnsi="Helvetica""#), "{document}");
        assert!(document.contains(r#"w:cs="Helvetica""#), "{document}");
        assert!(
            document.contains("The quick brown fox jumps over the lazy dog. 0123456789"),
            "{document}"
        );
    }

    #[test]
    fn a_face_name_that_would_break_the_attribute_is_escaped() {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(probe_document("A&B<\"x\">").unwrap()))
                .unwrap();
        let document = part(&mut archive, "word/document.xml");
        assert!(
            document.contains(r#"w:ascii="A&amp;B&lt;&quot;x&quot;&gt;""#),
            "{document}"
        );
        assert!(!document.contains("A&B<"), "{document}");
    }

    #[test]
    fn the_probe_document_states_a_content_type_for_its_body() {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(probe_document("Helvetica").unwrap()))
                .unwrap();
        let types = part(&mut archive, "[Content_Types].xml");
        assert!(
            types.contains(r#"PartName="/word/document.xml""#),
            "{types}"
        );
        let relationships = part(&mut archive, "_rels/.rels");
        assert!(
            relationships.contains(r#"Target="word/document.xml""#),
            "{relationships}"
        );
    }

    #[test]
    fn the_version_answer_is_read_as_a_version_and_a_build() {
        let answer = "LibreOffice 26.8.2.2 9dea406ab7695df6325b11b0e19fdca8db4b7276\n";
        let (version, build) = parse_version(answer).unwrap();
        assert_eq!(version, "26.8.2.2");
        assert_eq!(build, "9dea406ab7695df6325b11b0e19fdca8db4b7276");
    }

    #[test]
    fn an_answer_that_states_no_version_is_refused() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("LibreOffice"), None);
        assert_eq!(parse_version("some other program 1.2.3"), None);
    }

    #[test]
    fn the_installation_is_the_bundle_the_launcher_sits_in() {
        assert_eq!(
            installation_of(Path::new(
                "/Applications/LibreOffice.app/Contents/MacOS/soffice"
            )),
            PathBuf::from("/Applications/LibreOffice.app")
        );
    }

    #[test]
    fn the_installation_is_the_prefix_above_a_program_directory() {
        assert_eq!(
            installation_of(Path::new("/opt/libreoffice7.6/program/soffice")),
            PathBuf::from("/opt/libreoffice7.6")
        );
    }

    #[test]
    fn the_installation_falls_back_to_the_launchers_own_directory() {
        assert_eq!(
            installation_of(Path::new("/usr/bin/soffice")),
            PathBuf::from("/usr/bin")
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_child_that_outlives_its_bound_is_killed_with_the_processes_it_started() {
        let area = crate::TempArea::new("reap").expect("a temporary area");
        let pid_file = area.path().join("grandchild.pid");
        let mut command = Command::new("/bin/sh");
        // The shell leaves a grandchild that states its pid and holds the output pipe: a run that
        // killed only the launcher would leave it running, and the readers waiting on it.
        command
            .arg("-c")
            .arg("sleep 30 & echo $! > \"$1\"; wait")
            .arg("sh")
            .arg(&pid_file);
        let finished = run(command, Duration::from_secs(1)).expect("a bounded run");
        assert!(finished.timed_out, "the run did not reach its bound");
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .expect("the grandchild states its pid")
            .trim()
            .parse()
            .expect("a pid");
        // The group is killed after the leader, so the grandchild may be gone a moment after the run
        // returns; that it is gone is the property, not that time passed.
        let mut alive = true;
        for _ in 0..50 {
            // Safety: signal 0 asks whether the process exists, and sends nothing.
            alive = unsafe { libc::kill(pid, 0) } == 0;
            if !alive {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !alive,
            "the grandchild {pid} outlived the run that killed its launcher"
        );
    }

    #[test]
    fn find_answers_this_machines_installation_or_says_what_it_looked_for() {
        let bundle = Path::new("/Applications/LibreOffice.app/Contents/MacOS/soffice");
        match find() {
            Ok(reference) => {
                assert_eq!(reference.program, "LibreOffice");
                assert!(!reference.version.is_empty());
                assert!(reference.launcher.is_file(), "{:?}", reference.launcher);
            }
            Err(message) => {
                assert!(
                    !bundle.exists(),
                    "the launcher is there and find() refused: {message}"
                );
                assert!(
                    message.starts_with("the outside reference is not found"),
                    "{message}"
                );
            }
        }
    }

    #[test]
    fn a_profile_path_with_a_space_or_a_hash_is_percent_encoded() {
        let url = file_url(Path::new("/tmp/one run #2/profile"));
        assert_eq!(url, "file:///tmp/one%20run%20%232/profile");
        assert!(url.starts_with("file:///"), "{url}");
    }

    #[test]
    fn the_filter_comes_from_the_documents_extension() {
        assert_eq!(
            format_of(Path::new("/tmp/a.docx")).unwrap().filter(),
            "writer_pdf_Export"
        );
        assert_eq!(format_of(Path::new("/tmp/a.pptx")).unwrap(), Format::Pptx);
        assert_eq!(format_of(Path::new("/tmp/A.XLSX")).unwrap(), Format::Xlsx);
    }

    #[test]
    fn a_document_no_family_of_the_record_covers_is_refused() {
        let message = format_of(Path::new("/tmp/a.pdf")).unwrap_err();
        assert!(message.contains("\"pdf\""), "{message}");
        let message = format_of(Path::new("/tmp/a")).unwrap_err();
        assert!(message.contains("\"\""), "{message}");
    }

    #[test]
    fn a_converter_writes_the_locale_pin_and_the_font_configuration_before_any_run() {
        let area = crate::TempArea::new("converter").unwrap();
        let reference = Reference {
            program: "LibreOffice".to_string(),
            version: "26.8.2.2".to_string(),
            build: "9dea406ab7695df6325b11b0e19fdca8db4b7276".to_string(),
            installation: PathBuf::from("/Applications/LibreOffice.app"),
            launcher: PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS/soffice"),
        };
        let config = "<fontconfig>\n</fontconfig>\n";
        let converter = Converter::new(&reference, area.path(), config).unwrap();
        assert!(converter.profile.is_dir());
        assert!(converter.cache.is_dir());
        assert!(converter.tmp.is_dir());
        let pin = std::fs::read_to_string(converter.profile.join("user/registrymodifications.xcu"))
            .unwrap();
        assert!(pin.contains(&format!("<value>{LOCALE}</value>")), "{pin}");
        assert!(
            pin.contains("ooSetupSystemLocale"),
            "the pin states no locale setting: {pin}"
        );
        assert_eq!(std::fs::read_to_string(&converter.fonts).unwrap(), config);
    }

    #[test]
    fn a_conversion_refuses_a_directory_that_already_holds_its_output() {
        let area = crate::TempArea::new("output").unwrap();
        let reference = Reference {
            program: "LibreOffice".to_string(),
            version: "26.8.2.2".to_string(),
            build: "9dea406ab7695df6325b11b0e19fdca8db4b7276".to_string(),
            installation: PathBuf::from("/Applications/LibreOffice.app"),
            launcher: PathBuf::from("/nonexistent/soffice"),
        };
        let converter = Converter::new(&reference, area.path(), "<fontconfig/>\n").unwrap();
        let input = area.path().join("probe.docx");
        std::fs::write(&input, probe_document("Helvetica").unwrap()).unwrap();
        let out = area.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("probe.pdf"), b"%PDF-1.7 older").unwrap();
        let message = converter.convert(&input, &out).unwrap_err();
        assert!(message.contains("already holds a document"), "{message}");
    }
}
