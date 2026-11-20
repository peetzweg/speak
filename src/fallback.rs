//! When the model (or a required espeak-ng) is unavailable, the message still gets out through
//! the platform's own voice: `/usr/bin/say` on macOS, `spd-say` or `espeak-ng` on Linux.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Speak `text` (or write it to `output`) with the system voice. `rate` is words per minute.
pub fn speak(text: &str, rate: Option<f32>, output: Option<&Path>) -> Result<()> {
    if cfg!(target_os = "macos") {
        return say(text, rate, output);
    }
    if let Some(espeak) = crate::g2p::espeak::find_program() {
        let mut cmd = espeak.command();
        if let Some(r) = rate {
            cmd.arg(format!("-s{}", r.round() as u32));
        }
        if let Some(out) = output {
            if !out.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
                bail!("the fallback voice can only write .wav files");
            }
            cmd.arg("-w").arg(std::path::absolute(out)?); // the command may run in another directory
        }
        return run(cmd, text);
    }
    if output.is_none()
        && let Some(spd) = crate::paths::which("spd-say")
    {
        let mut cmd = Command::new(spd);
        cmd.args(["-w", "-e"]); // wait until spoken; text on stdin
        return run(cmd, text);
    }
    bail!("no system voice found (install espeak-ng or speech-dispatcher)")
}

fn say(text: &str, rate: Option<f32>, output: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new("/usr/bin/say");
    cmd.args(["-f", "-"]);
    if let Some(r) = rate {
        cmd.arg(format!("-r{}", r.round() as u32));
    }
    if let Some(out) = output {
        cmd.arg("-o").arg(out);
        // say infers most formats from the extension, but WAV needs an explicit data format.
        if out.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
            cmd.args(["--file-format=WAVE", "--data-format=LEI16@24000"]);
        }
    }
    run(cmd, text)
}

/// Run `cmd` with `text` on stdin (so no text is ever mistaken for a flag).
fn run(mut cmd: Command, text: &str) -> Result<()> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::null()).spawn().with_context(|| format!("running {program}"))?;
    crate::queue::set_child(Some(child.id())); // `speak stop` stops it too
    child.stdin.take().context("stdin")?.write_all(text.as_bytes())?;
    let status = child.wait();
    crate::queue::set_child(None);
    if !status?.success() {
        bail!("{program} failed");
    }
    Ok(())
}
