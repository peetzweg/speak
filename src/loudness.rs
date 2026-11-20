//! Loudness normalisation for written audio: ITU-R BS.1770-4 integrated loudness (K-weighting,
//! 400 ms blocks every 100 ms, the −70 LUFS absolute and −10 LU relative gates), then a gain to
//! the target with a look-ahead limiter that keeps sample peaks under a ceiling.

use crate::timeline::Loudness;

/// K-weighting for `rate`: the high shelf and high-pass of BS.1770, derived for any sample rate
/// from their analog prototypes (the constants libebur128 uses), combined into one 4th-order
/// filter as (b, a).
fn k_weighting(rate: u32) -> ([f64; 5], [f64; 5]) {
    let rate = f64::from(rate);
    let (f0, gain_db, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf_b = [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0];
    let shelf_a = [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0];
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp_b = [1.0, -2.0, 1.0];
    let hp_a = [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0];
    (convolve(shelf_b, hp_b), convolve(shelf_a, hp_a))
}

fn convolve(x: [f64; 3], y: [f64; 3]) -> [f64; 5] {
    let mut out = [0.0; 5];
    for (i, a) in x.iter().enumerate() {
        for (j, b) in y.iter().enumerate() {
            out[i + j] += a * b;
        }
    }
    out
}

/// Mean square of the K-weighted signal in each 100 ms step.
fn step_energies(samples: &[f32], rate: u32) -> Vec<f64> {
    let (b, a) = k_weighting(rate);
    let step = (rate as usize / 10).max(1);
    let (mut x, mut y) = ([0.0f64; 4], [0.0f64; 4]);
    let mut out = Vec::with_capacity(samples.len() / step + 1);
    let mut sum = 0.0;
    for (n, &s) in samples.iter().enumerate() {
        let s = f64::from(s);
        let v = b[0] * s + b[1] * x[0] + b[2] * x[1] + b[3] * x[2] + b[4] * x[3] - a[1] * y[0] - a[2] * y[1] - a[3] * y[2] - a[4] * y[3];
        x = [s, x[0], x[1], x[2]];
        y = [v, y[0], y[1], y[2]];
        sum += v * v;
        if (n + 1) % step == 0 {
            out.push(sum / step as f64);
            sum = 0.0;
        }
    }
    out
}

fn lufs(mean_square: f64) -> f64 {
    -0.691 + 10.0 * mean_square.log10()
}

/// Integrated loudness in LUFS of mono audio; `None` when nothing is above the absolute gate
/// (silence, or shorter than one 400 ms block).
pub fn integrated(samples: &[f32], rate: u32) -> Option<f64> {
    let steps = step_energies(samples, rate);
    let blocks: Vec<f64> = steps.windows(4).map(|w| w.iter().sum::<f64>() / 4.0).collect();
    let mean = |v: &[f64]| (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64);
    let above: Vec<f64> = blocks.iter().copied().filter(|&z| lufs(z) > -70.0).collect();
    let relative = lufs(mean(&above)?) - 10.0;
    let gated: Vec<f64> = above.into_iter().filter(|&z| lufs(z) > relative).collect();
    mean(&gated).map(lufs)
}

pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

pub fn db(amplitude: f64) -> f64 {
    20.0 * amplitude.max(1e-10).log10()
}

/// The minimum of `v` over a window of `radius` on each side.
fn sliding_min(v: &[f32], radius: usize) -> Vec<f32> {
    // van Herk/Gil-Werman: running minima over blocks of the window's width, from both ends.
    let w = 2 * radius + 1;
    let n = v.len();
    let padded = |i: usize| if i < radius || i - radius >= n { 1.0 } else { v[i - radius] };
    let len = n + 2 * radius;
    let (mut left, mut right) = (vec![0.0f32; len], vec![0.0f32; len]);
    for i in 0..len {
        left[i] = if i % w == 0 { padded(i) } else { left[i - 1].min(padded(i)) };
    }
    for i in (0..len).rev() {
        right[i] = if i % w == w - 1 || i == len - 1 { padded(i) } else { right[i + 1].min(padded(i)) };
    }
    (0..n).map(|i| right[i].min(left[i + w - 1])).collect()
}

/// The mean of `v` over a window of `radius` on each side (values past the ends count as 1).
fn sliding_mean(v: &[f32], radius: usize) -> Vec<f32> {
    let w = (2 * radius + 1) as f64;
    let at = |i: isize| if i < 0 || i as usize >= v.len() { 1.0 } else { f64::from(v[i as usize]) };
    let r = radius as isize;
    let mut sum: f64 = (-r..=r).map(at).sum();
    let mut out = Vec::with_capacity(v.len());
    for i in 0..v.len() as isize {
        out.push((sum / w) as f32);
        sum += at(i + r + 1) - at(i - r);
    }
    out
}

/// Scale `samples` by `gain`, then pull every peak above `ceiling` down with a smooth gain curve:
/// the needed reduction, spread by a window minimum and averaged over the same window, never lets
/// a sample through above the ceiling and changes gain over about 10 ms, not instantly.
fn gain_and_limit(samples: &[f32], gain: f32, ceiling: f32, rate: u32) -> Vec<f32> {
    let scaled: Vec<f32> = samples.iter().map(|s| s * gain).collect();
    if peak(&scaled) <= ceiling {
        return scaled;
    }
    let radius = (rate as usize / 200).max(1); // 5 ms
    let need: Vec<f32> = scaled.iter().map(|s| if s.abs() > ceiling { ceiling / s.abs() } else { 1.0 }).collect();
    let curve = sliding_mean(&sliding_min(&need, radius), radius);
    // Recover slowly (50 ms) after each reduction, so the gain doesn't flutter between syllables.
    let release = 1.0 - (-1.0 / (0.05 * rate as f32)).exp();
    let mut g = 1.0f32;
    scaled
        .iter()
        .zip(curve)
        .map(|(s, c)| {
            g = c.min(g + (1.0 - g) * release);
            (s * g).clamp(-ceiling, ceiling)
        })
        .collect()
}

/// Bring `samples` to `target` LUFS with sample peaks at most `ceiling_db` dBFS. The limiter
/// takes a little loudness off, so the gain is refined until the result is within 0.05 LU.
/// `None` (and nothing changed) when the audio is silent.
pub fn normalize(samples: &mut Vec<f32>, rate: u32, target: f64, ceiling_db: f64) -> Option<Loudness> {
    let before = integrated(samples, rate)?;
    let ceiling = 10f64.powf(ceiling_db / 20.0) as f32;
    let mut gain_db = target - before;
    let mut out = gain_and_limit(samples, 10f64.powf(gain_db / 20.0) as f32, ceiling, rate);
    let mut after = integrated(&out, rate).unwrap_or(before);
    for _ in 0..6 {
        if (after - target).abs() < 0.05 {
            break;
        }
        gain_db += target - after;
        out = gain_and_limit(samples, 10f64.powf(gain_db / 20.0) as f32, ceiling, rate);
        after = integrated(&out, rate).unwrap_or(after);
    }
    let peak_db = db(f64::from(peak(&out)));
    *samples = out;
    Some(Loudness { before, after, gain_db, peak_db })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, amp: f32, secs: f64, rate: u32) -> Vec<f32> {
        (0..(secs * f64::from(rate)) as usize)
            .map(|n| amp * (2.0 * std::f64::consts::PI * freq * n as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    #[test]
    fn k_weighting_matches_the_standard_at_48k() {
        // BS.1770-4, table 1 and 2 (the two stages, here combined).
        let (b, a) = k_weighting(48_000);
        let want_b = convolve([1.53512485958697, -2.69169618940638, 1.19839281085285], [1.0, -2.0, 1.0]);
        let want_a = convolve([1.0, -1.69065929318241, 0.73248077421585], [1.0, -1.99004745483398, 0.99007225036621]);
        for i in 0..5 {
            assert!((b[i] - want_b[i]).abs() < 1e-8 && (a[i] - want_a[i]).abs() < 1e-8, "{b:?} {a:?}");
        }
    }

    #[test]
    fn sine_at_1khz_reads_as_the_standard_says() {
        // A 1 kHz sine at 0 dBFS peak, one channel, is −3.01 LUFS (BS.1770's reference point).
        for rate in [24_000, 48_000] {
            let l = integrated(&sine(1000.0, 1.0, 5.0, rate), rate).unwrap();
            assert!((l - -3.01).abs() < 0.05, "{rate}: {l}");
            let l = integrated(&sine(1000.0, 0.1, 5.0, rate), rate).unwrap();
            assert!((l - -23.01).abs() < 0.05, "{rate}: {l}");
        }
        assert_eq!(integrated(&vec![0.0; 48_000], 24_000), None);
        assert_eq!(integrated(&sine(1000.0, 1.0, 0.3, 24_000), 24_000), None);
    }

    #[test]
    fn gating_ignores_pauses() {
        // Speech-like: tone, long silence, tone. The silence must not drag the reading down.
        let rate = 24_000;
        let mut x = sine(1000.0, 0.1, 2.0, rate);
        x.extend(vec![0.0; 10 * rate as usize]);
        x.extend(sine(1000.0, 0.1, 2.0, rate));
        // Ungated, the 10 s of silence would read 5.4 dB lower; the blocks across each edge of a
        // tone are partly silent and still pass the gates, so a little below the tone's -23.01.
        let l = integrated(&x, rate).unwrap();
        assert!((-23.5..-23.0).contains(&l), "{l}");
    }

    #[test]
    fn normalizes_with_a_peak_ceiling() {
        let rate = 24_000;
        // Quiet with one loud transient: reaching the target needs the limiter.
        let mut x = sine(440.0, 0.05, 6.0, rate);
        for (i, s) in x.iter_mut().enumerate().skip(30_000).take(2_000) {
            *s = if i % 2 == 0 { 0.9 } else { -0.9 };
        }
        let l = normalize(&mut x, rate, -16.0, -1.0).unwrap();
        assert!((l.after - -16.0).abs() < 0.1, "{l:?}");
        assert!(l.peak_db <= -1.0 + 1e-6, "{l:?}");
        assert!((integrated(&x, rate).unwrap() - l.after).abs() < 1e-9);
        // Silence is left alone.
        let mut quiet = vec![0.0; 48_000];
        assert_eq!(normalize(&mut quiet, rate, -16.0, -1.0), None);
    }

    #[test]
    fn window_helpers() {
        let v = [1.0, 0.5, 1.0, 1.0, 0.2, 1.0, 1.0];
        assert_eq!(sliding_min(&v, 1), vec![0.5, 0.5, 0.5, 0.2, 0.2, 0.2, 1.0]);
        let m = sliding_mean(&[0.0, 0.0, 0.0], 1);
        assert!((m[0] - 1.0 / 3.0).abs() < 1e-6 && m[1] == 0.0);
    }
}
