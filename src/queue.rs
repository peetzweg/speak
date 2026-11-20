//! Taking turns at the speakers: concurrent `speak` processes (several agents at once) play one
//! after another, in the order they started, never over each other.
//!
//! `<runtime dir>/queue/` holds a ticket for every process that waits to play or plays, named by
//! its start time and pid and locked by that process for as long as it lives. A process plays once
//! no earlier ticket is left, holding `play.lock` (which also names its pid) while it plays. The
//! OS drops a dead process's locks, so a crash leaves nothing stuck: whoever finds a ticket nobody
//! holds deletes it. Synthesis goes on while a process waits, so it speaks the moment its turn
//! comes. Files (`-o`) never queue.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A `--background` parent's start time, handed to its child so the child keeps that place in line.
pub const QUEUED_AT: &str = "SPEAK_QUEUED_AT";
const PLAY_LOCK: &str = "play.lock";

pub fn dir() -> PathBuf {
    crate::paths::runtime_dir().join("queue")
}

/// Now, as a ticket time: nanoseconds since the Unix epoch.
pub fn now() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

/// A place in line, held until dropped.
pub struct Ticket {
    path: PathBuf,
    name: String,
    _file: File,
}

/// The speakers, held until dropped.
pub struct Turn {
    _lock: File,
}

const IDLE: u8 = 0;
const QUEUED: u8 = 1;
const PLAYING: u8 = 2;
static STATE: AtomicU8 = AtomicU8::new(IDLE);
/// The system voice's process, while one speaks for us.
static CHILD: AtomicI32 = AtomicI32::new(0);

impl Ticket {
    /// Get in line (at `SPEAK_QUEUED_AT` if set, else now).
    pub fn take() -> io::Result<Ticket> {
        let at = std::env::var(QUEUED_AT).ok().and_then(|v| v.parse().ok()).unwrap_or_else(now);
        let dir = dir();
        fs::create_dir_all(&dir)?;
        let pid = std::process::id();
        let name = format!("{at:024}-{pid:010}");
        // Locked before it appears under its real name, so nobody takes it for a dead one.
        let tmp = dir.join(format!(".{name}"));
        let mut file = File::create(&tmp)?;
        file.lock()?;
        writeln!(file, "{pid}")?;
        fs::rename(&tmp, dir.join(&name))?;
        if std::env::var_os("SPEAK_DEBUG").is_some() {
            eprintln!("speak: in line as {name}");
        }
        STATE.store(QUEUED, Ordering::SeqCst);
        signals::install(&dir.join(&name));
        Ok(Ticket { path: dir.join(&name), name, _file: file })
    }

    /// Wait until everyone who got in line earlier is done, then take the speakers.
    pub fn wait_turn(&self) -> io::Result<Turn> {
        let dir = dir();
        loop {
            let earlier: Vec<PathBuf> = tickets(&dir)?.into_iter().filter(|(n, _)| *n < self.name).map(|(_, p)| p).collect();
            if earlier.is_empty() {
                break;
            }
            for t in earlier {
                wait_for(&t);
            }
        }
        let mut lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join(PLAY_LOCK))?;
        lock.lock()?;
        lock.set_len(0)?;
        write!(lock, "{}", std::process::id())?;
        STATE.store(PLAYING, Ordering::SeqCst);
        Ok(Turn { _lock: lock })
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        // Deleted while still locked: a waiter wakes to find it gone, not abandoned.
        let _ = fs::remove_file(&self.path);
        STATE.store(IDLE, Ordering::SeqCst);
    }
}

/// Tickets in line order (names sort by time).
fn tickets(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut out: Vec<(String, PathBuf)> = fs::read_dir(dir)?
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().into_string().ok()?;
            (!name.starts_with('.') && name != PLAY_LOCK).then(|| (name, e.path()))
        })
        .collect();
    out.sort();
    Ok(out)
}

/// Block until the process holding ticket `path` is gone. If it died without cleaning up, remove
/// its ticket.
fn wait_for(path: &Path) {
    let Ok(f) = File::open(path) else { return };
    if f.lock_shared().is_ok() {
        let _ = fs::remove_file(path);
    }
}

/// The pid in a lock file, if a live process holds the file.
fn holder(path: &Path) -> Option<u32> {
    let mut f = File::open(path).ok()?;
    match f.try_lock_shared() {
        Err(fs::TryLockError::WouldBlock) => {
            let mut s = String::new();
            f.read_to_string(&mut s).ok()?;
            s.trim().parse().ok()
        }
        _ => None,
    }
}

/// While the system voice speaks for this process, `speak stop` stops it too.
pub fn set_child(pid: Option<u32>) {
    CHILD.store(pid.and_then(|p| i32::try_from(p).ok()).unwrap_or(0), Ordering::SeqCst);
}

/// Playback was stopped: leave now, without waiting for synthesis still running (its model may
/// be mid-inference), so the next in line starts at once. Exit code 0.
pub fn exit_stopped() -> ! {
    signals::exit_now()
}

/// `speak stop`: stop the speech playing now (or, if none plays yet, the first in line) and, with
/// `all`, everything queued after it. Waits up to 3 s for them to go quiet. Returns how many
/// processes were stopped.
pub fn stop(all: bool) -> io::Result<usize> {
    let dir = dir();
    if !dir.is_dir() {
        return Ok(0);
    }
    // Each process to stop, with a lock it holds until it is gone.
    let lock = dir.join(PLAY_LOCK);
    let mut targets: Vec<(u32, PathBuf)> = holder(&lock).map(|pid| (pid, lock)).into_iter().collect();
    for (_, path) in tickets(&dir)? {
        match holder(&path) {
            Some(pid) if all || targets.is_empty() => targets.push((pid, path)),
            Some(_) => {}
            None => wait_for(&path), // dead: tidy up
        }
    }
    let mut pids: Vec<u32> = targets.iter().map(|t| t.0).filter(|&p| p != std::process::id()).collect();
    pids.sort_unstable();
    pids.dedup();
    pids.retain(|&p| signals::terminate(p));
    targets.retain(|t| pids.contains(&t.0));
    let deadline = Instant::now() + Duration::from_secs(3);
    while targets.iter().any(|(pid, path)| holder(path) == Some(*pid)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(pids.len())
}

/// Run `speak ARGS` detached from this process and its terminal, with `text` on its stdin, in this
/// process's place in line (`started`). Re-running the program rather than forking keeps this
/// possible on Windows.
pub fn spawn_background(args: &[OsString], text: &str, started: u128) -> io::Result<()> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.args(args).env(QUEUED_AT, started.to_string()).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    signals::detach(&mut cmd);
    let mut child = cmd.spawn()?;
    child.stdin.take().expect("piped stdin").write_all(text.as_bytes())
}

#[cfg(unix)]
mod signals {
    use std::ffi::CString;
    use std::path::Path;
    use std::process::Command;
    use std::sync::OnceLock;
    use std::sync::atomic::Ordering;

    use super::{CHILD, PLAYING, STATE};

    /// This process's ticket, for the handler to remove (one ticket per process).
    static TICKET: OnceLock<CString> = OnceLock::new();

    /// `speak stop` sends SIGTERM. While queued, the process just ends (exit 0: being stopped is
    /// not a failure); while playing, playback stops and it exits normally; the system voice is
    /// stopped with it.
    extern "C" fn on_term(_: libc::c_int) {
        let child = CHILD.load(Ordering::SeqCst);
        if child > 0 {
            // SAFETY: kill and _exit are async-signal-safe.
            unsafe {
                libc::kill(child, libc::SIGTERM);
                libc::_exit(0);
            }
        }
        if STATE.load(Ordering::SeqCst) == PLAYING {
            crate::audio::STOP.store(true, Ordering::SeqCst);
            return;
        }
        // SAFETY: unlink and _exit are async-signal-safe; _exit skips destructors (ONNX Runtime
        // may be loading a model).
        unsafe {
            if let Some(t) = TICKET.get() {
                libc::unlink(t.as_ptr());
            }
            libc::_exit(0)
        }
    }

    pub fn install(ticket: &Path) {
        use std::os::unix::ffi::OsStrExt;
        if let Ok(t) = CString::new(ticket.as_os_str().as_bytes()) {
            let _ = TICKET.set(t);
        }
        let handler: extern "C" fn(libc::c_int) = on_term;
        // SAFETY: the handler only touches atomics and async-signal-safe calls.
        unsafe { libc::signal(libc::SIGTERM, handler as libc::sighandler_t) };
    }

    pub fn exit_now() -> ! {
        // SAFETY: as in the handler.
        unsafe {
            if let Some(t) = TICKET.get() {
                libc::unlink(t.as_ptr());
            }
            libc::_exit(0)
        }
    }

    pub fn terminate(pid: u32) -> bool {
        // SAFETY: plain syscall.
        i32::try_from(pid).is_ok_and(|p| unsafe { libc::kill(p, libc::SIGTERM) } == 0)
    }

    /// A new session: no controlling terminal, so closing it (or the agent's shell) doesn't end the
    /// speech.
    pub fn detach(cmd: &mut Command) {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe, as pre_exec requires.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            })
        };
    }
}

#[cfg(not(unix))]
mod signals {
    //! Not yet: Windows would stop processes through a named event and detach with
    //! DETACHED_PROCESS; the queue itself is portable.
    use std::process::Command;

    pub fn install(_ticket: &std::path::Path) {}
    pub fn exit_now() -> ! {
        std::process::exit(0)
    }
    pub fn terminate(_pid: u32) -> bool {
        false
    }
    pub fn detach(_cmd: &mut Command) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_tickets_are_cleared_and_live_ones_waited_for() {
        let dir = std::env::temp_dir().join(format!("speak-queue-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // A ticket nobody holds (its process crashed): waiting for it returns at once and removes it.
        let dead = dir.join("000000000000000000000001-0000000001");
        fs::write(&dead, "1\n").unwrap();
        assert_eq!(holder(&dead), None);
        wait_for(&dead);
        assert!(!dead.exists());

        // A held one is waited for until its holder lets go.
        let live = dir.join("000000000000000000000002-0000000002");
        let f = File::create(&live).unwrap();
        f.lock().unwrap();
        fs::write(&live, "4242\n").unwrap();
        assert_eq!(holder(&live), Some(4242));
        let path = live.clone();
        let waiter = std::thread::spawn(move || {
            let t = Instant::now();
            wait_for(&path);
            t.elapsed()
        });
        std::thread::sleep(Duration::from_millis(200));
        fs::remove_file(&live).unwrap();
        drop(f);
        assert!(waiter.join().unwrap() >= Duration::from_millis(150));
        assert_eq!(tickets(&dir).unwrap(), vec![]);
        let _ = fs::remove_dir_all(&dir);
    }
}
