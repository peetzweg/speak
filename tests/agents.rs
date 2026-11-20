//! Live use by agents: concurrent speak processes take turns, `speak stop`, crashed speakers and
//! `--background`. Playback goes to `SPEAK_AUDIO=null` (real time, no sound). Tests that need the
//! Kokoro model skip (with a note) when it isn't available.

mod common;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use common::*;

const LONG: &str = "This is the first of several sentences, long enough to keep playing for a while. \
It goes on for a bit longer. And then a little more, so there is time to stop it. And one more.";

/// A speak process whose stderr (with SPEAK_DEBUG) is read as it comes.
struct Speaker {
    child: Child,
    lines: Receiver<String>,
    log: Vec<String>,
}

impl Speaker {
    fn start(home: &Path, run_dir: &Path, args: &[&str]) -> Speaker {
        let mut cmd = speaker(home, run_dir);
        let mut child = cmd.env("SPEAK_DEBUG", "1").args(args).stderr(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let (tx, lines) = channel();
        let err = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        Speaker { child, lines, log: Vec::new() }
    }

    /// Wait (up to 60 s) for a stderr line containing `what`.
    fn wait_for(&mut self, what: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        if let Some(l) = self.log.iter().find(|l| l.contains(what)) {
            return l.clone();
        }
        while let Ok(line) = self.lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            self.log.push(line.clone());
            if line.contains(what) {
                return line;
            }
        }
        panic!("no '{what}' from speak; got {:?}", self.log);
    }

    /// Exit code and the whole stderr log.
    fn finish(mut self) -> (Option<i32>, Vec<String>, Vec<u8>) {
        let mut out = Vec::new();
        std::io::Read::read_to_end(self.child.stdout.as_mut().unwrap(), &mut out).unwrap();
        let status = self.child.wait().unwrap();
        self.log.extend(self.lines.iter());
        (status.code(), self.log, out)
    }
}

/// The `at` time (unix ms) in a "playback starts at …" / "playback ends at …" line.
fn at(line: &str) -> u128 {
    line.split(" at ").nth(1).unwrap().chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap()
}

fn unix_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
}

/// speak with null audio and a playback queue shared by every process of one test.
fn speaker(home: &Path, run_dir: &Path) -> Command {
    let mut cmd = speak(Some(home));
    cmd.env("XDG_RUNTIME_DIR", run_dir).env("SPEAK_AUDIO", "null");
    cmd
}

fn queue_dir(run_dir: &Path) -> PathBuf {
    run_dir.join("speak/queue")
}

fn tickets(run_dir: &Path) -> Vec<String> {
    std::fs::read_dir(queue_dir(run_dir))
        .map(|d| d.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| n != "play.lock").collect())
        .unwrap_or_default()
}

/// The first exec of a freshly built binary can be held up by the OS (macOS scans it), which would
/// make the processes of a test start together instead of in order.
fn warm_up() {
    let _ = run(speak(None).arg("--version"));
}

#[test]
fn concurrent_speakers_take_turns_and_files_never_wait() {
    let Some(home) = model_home() else { return };
    warm_up();
    let run_dir = tmpdir("turns");
    let mut a = Speaker::start(&home, &run_dir, &[LONG]);
    a.wait_for("in line as");
    let mut b = Speaker::start(&home, &run_dir, &["Second in line."]);
    b.wait_for("in line as");
    // A file is written while A still has the speakers.
    let file = tmpdir("turns-file").join("f.wav");
    let o = run(speaker(&home, &run_dir).arg("-o").arg(&file).arg("A file, written at once."));
    assert!(o.status.success(), "{}", stderr(&o));
    let file_done = unix_ms();

    let a_end = at(&a.wait_for("playback ends at"));
    let a_start = at(&a.wait_for("playback starts at"));
    let b_start = at(&b.wait_for("playback starts at"));
    let b_sound: u128 = b.wait_for("playback ends at").split("first sound at ").nth(1).unwrap().parse().unwrap();
    let (code_a, _, out_a) = a.finish();
    let (code_b, log_b, out_b) = b.finish();
    assert_eq!((code_a, code_b), (Some(0), Some(0)), "{log_b:?}");
    assert!(out_a.is_empty() && out_b.is_empty(), "stdout stays empty");
    assert!(file_done < a_end, "-o waited for the speakers");
    assert!(b_start >= a_end, "B started at {b_start}, before A ended at {a_end}");
    // B synthesised while it waited, so it speaks the moment A is done.
    assert!(b_sound >= a_end && b_sound - a_end < 300, "B sounded {} ms after A ended", b_sound as i128 - a_end as i128);
    assert!(a_end - a_start > 3000, "A played only {} ms", a_end - a_start);
    assert!(tickets(&run_dir).is_empty(), "{:?}", tickets(&run_dir));
}

#[test]
fn stop_and_stop_all() {
    let Some(home) = model_home() else { return };
    warm_up();
    let run_dir = tmpdir("stop");
    let mut a = Speaker::start(&home, &run_dir, &[LONG]);
    a.wait_for("playback starts");
    let mut b = Speaker::start(&home, &run_dir, &[LONG]);
    b.wait_for("in line as");
    let mut c = Speaker::start(&home, &run_dir, &["Third in line."]);
    c.wait_for("in line as");
    std::thread::sleep(Duration::from_millis(1500)); // A is speaking

    let o = run(speaker(&home, &run_dir).arg("stop"));
    assert!(o.status.success() && o.stdout.is_empty() && o.stderr.is_empty(), "{}", stderr(&o));
    assert!(a.wait_for("playback ends").ends_with("(stopped)"));
    let (code, _, _) = a.finish();
    assert_eq!(code, Some(0), "a stopped speak exits 0");

    // B is next; --all stops it and C, which never gets to play.
    b.wait_for("playback starts");
    let o = run(speaker(&home, &run_dir).args(["stop", "--all"]));
    assert!(o.status.success(), "{}", stderr(&o));
    let (code_b, log_b, _) = b.finish();
    let (code_c, log_c, _) = c.finish();
    assert_eq!((code_b, code_c), (Some(0), Some(0)));
    assert!(log_b.iter().any(|l| l.ends_with("(stopped)")), "{log_b:?}");
    assert!(!log_c.iter().any(|l| l.contains("playback starts")), "{log_c:?}");
    assert!(tickets(&run_dir).is_empty(), "{:?}", tickets(&run_dir));

    // Nothing playing: stop is a quiet no-op.
    assert!(run(speaker(&home, &run_dir).args(["stop", "--all"])).status.success());
}

#[test]
fn a_crashed_speaker_blocks_nobody() {
    let Some(home) = model_home() else { return };
    warm_up();
    let run_dir = tmpdir("crash");
    let mut a = Speaker::start(&home, &run_dir, &[LONG]);
    a.wait_for("playback starts");
    std::thread::sleep(Duration::from_millis(500));
    a.child.kill().unwrap(); // SIGKILL: no clean-up at all
    a.child.wait().unwrap();
    assert_eq!(tickets(&run_dir).len(), 1, "the dead speaker's ticket is left behind");
    // A ticket from long ago whose process is gone, too.
    std::fs::write(queue_dir(&run_dir).join("000000000000000000000001-0000000001"), "1\n").unwrap();

    let t = Instant::now();
    let o = run(speaker(&home, &run_dir).arg("Still here."));
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(t.elapsed() < Duration::from_secs(30), "waited {:?}", t.elapsed());
    assert!(tickets(&run_dir).is_empty(), "{:?}", tickets(&run_dir));
}

#[test]
fn background_returns_at_once() {
    let Some(home) = model_home() else { return };
    warm_up();
    let run_dir = tmpdir("background");
    let dir = tmpdir("background-out");
    let file = dir.join("bg.wav");
    let srt = dir.join("bg.srt");
    let t = Instant::now();
    let o = run(speaker(&home, &run_dir)
        .arg("--background")
        .arg("-o")
        .arg(&file)
        .arg("--subtitles")
        .arg(&srt)
        .arg("From the background. Two sentences."));
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(o.stdout.is_empty() && o.stderr.is_empty());
    assert!(t.elapsed() < Duration::from_secs(3), "took {:?}", t.elapsed());
    let deadline = Instant::now() + Duration::from_secs(60);
    while !srt.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(read_wav(&file).3.len() > 24_000);
    assert!(std::fs::read_to_string(&srt).unwrap().contains("From the background."));

    // Mistakes are reported before detaching, with the usual exit codes.
    let o = run(speaker(&home, &run_dir).args(["--background", "-v", "xx_nobody", "Hi."]));
    assert_eq!(o.status.code(), Some(4), "{}", stderr(&o));
    let o = run(speaker(&home, &run_dir).args(["--background", "Hi [[voice xx_nobody]] there."]));
    assert_eq!(o.status.code(), Some(4), "{}", stderr(&o));

    // A background speaker queues like any other, and speak stop reaches it.
    let o = run(speaker(&home, &run_dir).args(["--background", LONG]));
    assert!(o.status.success(), "{}", stderr(&o));
    let deadline = Instant::now() + Duration::from_secs(30);
    while tickets(&run_dir).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(tickets(&run_dir).len(), 1);
    assert!(run(speaker(&home, &run_dir).args(["stop", "--all"])).status.success());
    assert!(tickets(&run_dir).is_empty(), "{:?}", tickets(&run_dir));
}
