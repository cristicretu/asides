//! What's said during a Measure & Annotate session comes in takes (each press of the talk key, the
//! microphone or the Asides shortcut held). When the session is done they're joined into one recording,
//! so what was said and what was annotated make one aside: the takes in order with a pause between
//! them (each its own paragraph), and each moment of the session placed on that recording, so a
//! annotation sits among what was said when it was made.

use crate::resample::resample;

/// The silence between two takes, longer than a paragraph break in the transcript.
pub const PAUSE_SECS: f32 = 2.0;

/// One take: when it started, in seconds into the session (negative when it was under way before
/// the session opened), and what it recorded.
pub struct Take<'a> {
    pub start: f32,
    pub samples: &'a [f32],
    pub sample_rate: u32,
}

/// The takes as one recording, at the first take's rate.
pub struct Joined {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    /// Each take, in order: (its start in the session, its start in the recording, its length),
    /// in seconds.
    spans: Vec<(f32, f32, f32)>,
}

pub fn join(takes: &[Take]) -> Joined {
    let mut order: Vec<&Take> = takes.iter().collect();
    order.sort_by(|a, b| a.start.total_cmp(&b.start));
    let sample_rate = order.first().map_or(16_000, |t| t.sample_rate.max(1));
    let rate = sample_rate as f32;
    let mut samples: Vec<f32> = Vec::new();
    let mut spans = Vec::new();
    for (i, take) in order.into_iter().enumerate() {
        if i > 0 {
            samples.resize(samples.len() + (PAUSE_SECS * rate) as usize, 0.0);
        }
        let audio = samples.len() as f32 / rate;
        let take_samples = resample(take.samples, take.sample_rate.max(1), sample_rate);
        spans.push((take.start, audio, take_samples.len() as f32 / rate));
        samples.extend(take_samples);
    }
    Joined { samples, sample_rate, spans }
}

impl Joined {
    /// Where a moment of the session (seconds into it) falls in the recording: in a take, at the
    /// same point of it; between two, along the pause between them, in order; before the first or
    /// after the last, as far before or after.
    pub fn offset(&self, time: f32) -> f32 {
        let Some(&(first, ..)) = self.spans.first() else { return time };
        if time < first {
            return time - first;
        }
        // Where the previous take ended, in the session and in the recording.
        let mut previous = (first, 0.0);
        for &(start, audio, length) in &self.spans {
            if time < start {
                let (end, audio_end) = previous;
                let along = ((time - end) / (start - end)).clamp(0.0, 1.0);
                return audio_end + along * (audio - audio_end);
            }
            if time <= start + length {
                return audio + (time - start);
            }
            previous = (start + length, audio + length);
        }
        previous.1 + (time - previous.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_takes_with_a_pause_and_places_moments() {
        let (a, b) = (vec![0.5; 32_000], vec![0.25; 8_000]);
        // Out of order, and the second at another rate.
        let joined = join(&[
            Take { start: 10.0, samples: &b, sample_rate: 8_000 },
            Take { start: 1.0, samples: &a, sample_rate: 16_000 },
        ]);
        assert_eq!(joined.sample_rate, 16_000);
        // 2 s, the pause, then 1 s.
        assert_eq!(joined.samples.len(), 16_000 * 5);
        assert_eq!((joined.samples[0], joined.samples[16_000 * 3]), (0.5, 0.0));
        assert!((joined.samples[16_000 * 9 / 2] - 0.25).abs() < 0.01);
        let at = |t: f32| joined.offset(t);
        // Before the first take, in it, at its end, along the pause, in the second, after it.
        assert_eq!([at(0.0), at(2.0), at(3.0), at(6.5), at(10.5), at(12.0)], [-1.0, 1.0, 2.0, 3.0, 4.5, 6.0]);
        assert!(at(3.5) < at(6.5) && at(6.5) < at(9.9) && at(9.9) < at(10.0));
    }

    #[test]
    fn nothing_said() {
        let joined = join(&[]);
        assert!(joined.samples.is_empty());
        assert_eq!(joined.offset(4.0), 4.0);
    }
}
