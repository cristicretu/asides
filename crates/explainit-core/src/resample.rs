//! Offline band-limited resampling (windowed sinc) of a finished mono recording to Whisper's 16 kHz.

use std::f64::consts::PI;

/// Zero crossings of the sinc kernel on each side, at the output rate.
const ZERO_CROSSINGS: f64 = 16.0;
/// Kernel table resolution per input sample.
const OVERSAMPLE: usize = 512;

pub fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    // Low-pass just below the lower Nyquist frequency, in cycles per input sample.
    let cutoff = 0.5 * ratio.min(1.0) * 0.94;
    let half_width = ZERO_CROSSINGS / ratio.min(1.0);
    let table = kernel_table(cutoff, half_width);

    let out_len = (input.len() as f64 * ratio).round() as usize;
    let reach = half_width.ceil() as i64;
    let last = input.len() as i64 - 1;
    (0..out_len)
        .map(|n| {
            let t = n as f64 / ratio;
            let center = t.floor() as i64;
            let mut acc = 0.0f64;
            for k in (center - reach + 1).max(0)..=(center + reach).min(last) {
                let distance = (t - k as f64).abs();
                if distance < half_width {
                    acc += input[k as usize] as f64 * lookup(&table, distance);
                }
            }
            acc as f32
        })
        .collect()
}

/// `table[i]` is the kernel value at distance `i / OVERSAMPLE` input samples.
fn kernel_table(cutoff: f64, half_width: f64) -> Vec<f64> {
    let len = (half_width * OVERSAMPLE as f64).ceil() as usize + 2;
    (0..len)
        .map(|i| {
            let d = i as f64 / OVERSAMPLE as f64;
            if d >= half_width {
                return 0.0;
            }
            let x = 2.0 * cutoff * d;
            let sinc = if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) };
            let u = d / half_width;
            let blackman = 0.42 + 0.5 * (PI * u).cos() + 0.08 * (2.0 * PI * u).cos();
            2.0 * cutoff * sinc * blackman
        })
        .collect()
}

fn lookup(table: &[f64], distance: f64) -> f64 {
    let pos = distance * OVERSAMPLE as f64;
    let i = pos as usize;
    let frac = pos - i as f64;
    table[i] * (1.0 - frac) + table[i + 1] * frac
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f64, rate: u32, secs: f64) -> Vec<f32> {
        (0..(rate as f64 * secs) as usize).map(|i| (2.0 * PI * freq * i as f64 / rate as f64).sin() as f32).collect()
    }

    /// RMS of the middle half, avoiding edge effects.
    fn rms(x: &[f32]) -> f64 {
        let mid = &x[x.len() / 4..x.len() * 3 / 4];
        (mid.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / mid.len() as f64).sqrt()
    }

    fn zero_crossings_per_sec(x: &[f32], rate: u32) -> f64 {
        let crossings = x.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        crossings as f64 / 2.0 / (x.len() as f64 / rate as f64)
    }

    #[test]
    fn keeps_speech_band_tones() {
        for from in [48_000, 44_100, 24_000] {
            let out = resample(&tone(1000.0, from, 1.0), from, 16_000);
            assert_eq!(out.len(), 16_000, "from {from}");
            assert!((rms(&out) - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.02, "from {from}: rms {}", rms(&out));
            assert!((zero_crossings_per_sec(&out, 16_000) - 1000.0).abs() < 5.0, "from {from}");
        }
    }

    #[test]
    fn removes_content_above_new_nyquist() {
        let out = resample(&tone(12_000.0, 48_000, 0.5), 48_000, 16_000);
        assert!(rms(&out) < 0.01, "aliased energy {}", rms(&out));
    }

    #[test]
    fn identity_when_rates_match() {
        let input = tone(440.0, 16_000, 0.1);
        assert_eq!(resample(&input, 16_000, 16_000), input);
    }
}
