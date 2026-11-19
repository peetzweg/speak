//! Time misaki-rs construction and per-sentence G2P; print phonemes for each argument.
use std::time::Instant;

fn main() {
    let t = Instant::now();
    let g2p = misaki_rs::G2P::new(misaki_rs::Language::EnglishUS);
    let init = t.elapsed();
    for text in std::env::args().skip(1) {
        let t = Instant::now();
        let r = g2p.g2p(&text);
        let dt = t.elapsed();
        match r {
            Ok((ps, _)) => println!("{:.1} ms  {ps}", dt.as_secs_f64() * 1e3),
            Err(e) => println!("error: {e}"),
        }
    }
    eprintln!("init {:.0} ms", init.as_secs_f64() * 1e3);
}
