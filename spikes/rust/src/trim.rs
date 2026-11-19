//! Port of the `librosa.effects.trim` copy that kokoro-onnx applies to every batch
//! (top_db=60, ref=max, frame_length=2048, hop_length=512, centred RMS with zero padding).

const FRAME: usize = 2048;
const HOP: usize = 512;
const TOP_DB: f32 = 60.0;

pub fn trim(y: &[f32]) -> &[f32] {
    // rms(): pad frame/2 zeros on both sides, frame, mean of squares, sqrt.
    let pad = FRAME / 2;
    let padded_len = y.len() + 2 * pad;
    if padded_len < FRAME {
        return y;
    }
    let n_frames = 1 + (padded_len - FRAME) / HOP;
    let sample = |i: usize| -> f32 { if i < pad || i >= pad + y.len() { 0.0 } else { y[i - pad] } };
    let rms: Vec<f32> = (0..n_frames)
        .map(|f| {
            let s: f64 = (f * HOP..f * HOP + FRAME).map(|i| (sample(i) as f64).powi(2)).sum();
            ((s / FRAME as f64) as f32).sqrt()
        })
        .collect();
    // amplitude_to_db(rms, ref=max) = 10*log10(max(1e-10, rms^2)) - 10*log10(max(1e-10, max^2))
    let max = rms.iter().cloned().fold(0.0f32, f32::max);
    let db = |a: f32| 10.0 * (a * a).max(1e-10).log10();
    let ref_db = db(max);
    let non_silent: Vec<usize> = rms.iter().enumerate().filter(|(_, a)| db(**a) - ref_db > -TOP_DB).map(|(i, _)| i).collect();
    match (non_silent.first(), non_silent.last()) {
        (Some(&first), Some(&last)) => {
            let start = first * HOP;
            let end = ((last + 1) * HOP).min(y.len());
            &y[start.min(end)..end]
        }
        _ => &y[0..0],
    }
}
