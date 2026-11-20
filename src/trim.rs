//! Silence trimming as kokoro-onnx applies it to every batch: librosa's `effects.trim` algorithm
//! (top_db=60 below the loudest frame, frame 2048, hop 512, centred RMS with zero padding).

const FRAME: usize = 2048;
const HOP: usize = 512;
const TOP_DB: f32 = 60.0;

/// The part of `y` to keep, as a sample range.
pub fn bounds(y: &[f32]) -> std::ops::Range<usize> {
    let pad = FRAME / 2;
    let padded_len = y.len() + 2 * pad;
    if padded_len < FRAME {
        return 0..y.len();
    }
    let n_frames = 1 + (padded_len - FRAME) / HOP;
    let sample = |i: usize| if i < pad || i >= pad + y.len() { 0.0 } else { y[i - pad] };
    let rms: Vec<f32> = (0..n_frames)
        .map(|f| {
            let sum: f64 = (f * HOP..f * HOP + FRAME).map(|i| (sample(i) as f64).powi(2)).sum();
            ((sum / FRAME as f64) as f32).sqrt()
        })
        .collect();
    let db = |a: f32| 10.0 * (a * a).max(1e-10).log10();
    let reference = db(rms.iter().copied().fold(0.0f32, f32::max));
    let loud: Vec<usize> = rms.iter().enumerate().filter(|(_, a)| db(**a) - reference > -TOP_DB).map(|(i, _)| i).collect();
    match (loud.first(), loud.last()) {
        (Some(&first), Some(&last)) => {
            let end = ((last + 1) * HOP).min(y.len());
            (first * HOP).min(end)..end
        }
        _ => 0..0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_leading_and_trailing_silence() {
        let mut y = vec![0.0f32; 24_000];
        for (i, s) in y.iter_mut().enumerate().skip(8_000).take(8_000) {
            *s = (i as f32 * 0.05).sin() * 0.5;
        }
        let t = &y[bounds(&y)];
        assert!(t.len() < y.len() && t.len() >= 8_000, "{}", t.len());
        // Frame-aligned, and it keeps all of the tone.
        let start = t.as_ptr() as usize - y.as_ptr() as usize;
        assert!(start / 4 <= 8_000 && (start / 4 + t.len()) >= 16_000);
        assert_eq!(bounds(&y), start / 4..start / 4 + t.len());
        // Pure silence has nothing quieter than itself, so librosa keeps all of it.
        assert_eq!(bounds(&[0.0; 10_000]), 0..10_000);
        assert_eq!(bounds(&[]), 0..0);
    }
}
