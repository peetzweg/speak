//! ONNX Runtime set-up shared by every model speak runs (Kokoro, the English G2P network).

use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use ort::environment::Environment;
use ort::session::Session;
use ort::session::builder::{AutoDevicePolicy, SessionBuilder};

/// The process's ONNX Runtime environment. ort allows only one per process, so every session is
/// built on this one, and it lives until the process exits.
pub fn environment() -> Result<&'static Environment> {
    static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| ort::init().with_name("speak").build().map_err(|e| e.to_string())).as_ref().map_err(|e| anyhow!("{e}"))
}

/// A session builder on the CPU provider, with `threads` intra-op threads (0: ONNX Runtime's
/// default). ort's default device policy registers CoreML on macOS: seconds of extra load time and
/// numbers that drift from the references. `SPEAK_EP=auto` brings it back.
pub fn cpu_session(threads: usize) -> Result<SessionBuilder> {
    let policy = match std::env::var("SPEAK_EP").as_deref() {
        Ok("auto") => AutoDevicePolicy::MaxEfficiency,
        _ => AutoDevicePolicy::PreferCPU,
    };
    let builder = Session::builder(environment()?)?.with_auto_device(policy).map_err(|e| anyhow!("{e}"))?;
    Ok(if threads > 0 { builder.with_intra_threads(threads).map_err(|e| anyhow!("{e}"))? } else { builder })
}
