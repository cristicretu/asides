//! Whisper models (download, verify, load) and transcription.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use explainit_core::markdown::Segment;
use explainit_core::models::ModelSpec;
use sha2::{Digest, Sha256};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

#[derive(Debug, Clone, PartialEq)]
pub enum ModelState {
    Ready,
    Downloading { progress: f32 },
    Failed(String),
}

/// Tracks each model's availability; downloads missing ones in the background.
pub struct ModelManager {
    states: Mutex<HashMap<&'static str, ModelState>>,
    changed: Condvar,
    notify: Box<dyn Fn(&'static str, ModelState) + Send + Sync>,
}

impl ModelManager {
    pub fn new(notify: impl Fn(&'static str, ModelState) + Send + Sync + 'static) -> Arc<ModelManager> {
        Arc::new(ModelManager { states: Mutex::new(HashMap::new()), changed: Condvar::new(), notify: Box::new(notify) })
    }

    /// Makes sure the model is on disk, starting a download if it is not (and none is running).
    pub fn ensure(self: &Arc<Self>, id: &str) {
        let spec = ModelSpec::find(id);
        let mut states = self.states.lock().unwrap();
        if matches!(states.get(spec.id), Some(ModelState::Ready | ModelState::Downloading { .. })) {
            return;
        }
        if spec.path().is_file() {
            states.insert(spec.id, ModelState::Ready);
            drop(states);
            self.set(spec.id, ModelState::Ready);
            return;
        }
        states.insert(spec.id, ModelState::Downloading { progress: 0.0 });
        drop(states);
        self.set(spec.id, ModelState::Downloading { progress: 0.0 });
        let this = self.clone();
        std::thread::spawn(move || {
            let result =
                download(spec, &spec.path(), |progress| this.set(spec.id, ModelState::Downloading { progress }));
            this.set(
                spec.id,
                match result {
                    Ok(()) => ModelState::Ready,
                    Err(err) => ModelState::Failed(format!("Could not download the speech model: {err:#}")),
                },
            );
        });
    }

    /// Blocks until the model is ready (or its download failed) and returns its path.
    pub fn wait_ready(self: &Arc<Self>, id: &str) -> Result<PathBuf> {
        let spec = ModelSpec::find(id);
        let failed = matches!(self.states.lock().unwrap().get(spec.id), Some(ModelState::Failed(_)));
        if failed {
            // Retry a failed download.
            self.states.lock().unwrap().remove(spec.id);
        }
        self.ensure(spec.id);
        let mut states = self.states.lock().unwrap();
        loop {
            match states.get(spec.id) {
                Some(ModelState::Ready) => return Ok(spec.path()),
                Some(ModelState::Failed(msg)) => bail!("{msg}"),
                _ => states = self.changed.wait(states).unwrap(),
            }
        }
    }

    fn set(&self, id: &'static str, state: ModelState) {
        self.states.lock().unwrap().insert(id, state.clone());
        self.changed.notify_all();
        (self.notify)(id, state);
    }
}

fn download(spec: &ModelSpec, path: &Path, progress: impl Fn(f32)) -> Result<()> {
    fs::create_dir_all(path.parent().context("model path has no parent")?)?;
    let partial = path.with_extension("part");
    let response = ureq::get(&spec.url()).call().context("request failed")?;
    let mut reader = response.into_body().into_with_config().limit(u64::MAX).reader();
    let mut file = File::create(&partial)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut last_report = Instant::now();
    loop {
        let n = reader.read(&mut buf).context("download interrupted")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if last_report.elapsed() > Duration::from_millis(500) {
            progress(done as f32 / spec.size as f32);
            last_report = Instant::now();
        }
    }
    file.sync_all()?;
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if digest != spec.sha256 {
        let _ = fs::remove_file(&partial);
        bail!("checksum mismatch for {}", spec.file);
    }
    fs::rename(&partial, path)?;
    Ok(())
}

pub struct Transcriber {
    pub model_id: String,
    context: WhisperContext,
    threads: i32,
}

impl Transcriber {
    pub fn load(model_id: &str, path: &Path) -> Result<Transcriber> {
        whisper_rs::install_logging_hooks();
        let path = path.to_str().context("model path is not UTF-8")?;
        let context = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .context("could not load the speech model")?;
        let threads = std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4) as i32;
        Ok(Transcriber { model_id: model_id.to_string(), context, threads })
    }

    /// Verbatim transcription of 16 kHz mono audio, in the most likely of `languages` (empty: any).
    pub fn transcribe(&self, pcm: &[f32], languages: &[String]) -> Result<Vec<Segment>> {
        // Whisper needs at least a second of audio; pad short clips with silence.
        let mut padded;
        let pcm = if pcm.len() < 17_600 {
            padded = pcm.to_vec();
            padded.resize(17_600, 0.0);
            &padded[..]
        } else {
            pcm
        };
        let mut state = self.context.create_state()?;
        let language = match languages {
            [only] => only.clone(),
            _ => {
                state.pcm_to_mel(pcm, self.threads as usize)?;
                let (_, probabilities) = state.lang_detect(0, self.threads as usize)?;
                choose_language(&probabilities, languages)?
            }
        };
        if whisper_rs::get_lang_id(&language).is_none() {
            bail!("Unknown language \"{language}\" in config.toml (use Whisper codes such as \"en\" or \"ro\")");
        }

        let mut params = FullParams::new(SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 });
        params.set_language(Some(&language));
        params.set_n_threads(self.threads);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_suppress_nst(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state.full(params, pcm)?;

        let segments = state
            .as_iter()
            .map(|s| -> Result<Segment> {
                Ok(Segment {
                    start: s.start_timestamp() as f32 / 100.0,
                    end: s.end_timestamp() as f32 / 100.0,
                    text: s.to_str_lossy()?.into_owned(),
                })
            })
            .collect::<Result<_>>()?;
        Ok(segments)
    }
}

/// The most likely language among `allowed` (any language Whisper knows when empty).
fn choose_language(probabilities: &[f32], allowed: &[String]) -> Result<String> {
    let probability = |id: i32| probabilities.get(id as usize).copied().unwrap_or(0.0);
    let candidates: Vec<i32> = if allowed.is_empty() {
        (0..=whisper_rs::get_lang_max_id()).collect()
    } else {
        allowed
            .iter()
            .map(|code| {
                whisper_rs::get_lang_id(code).with_context(|| {
                    format!("Unknown language \"{code}\" in config.toml (use Whisper codes such as \"en\" or \"ro\")")
                })
            })
            .collect::<Result<_>>()?
    };
    let best =
        candidates.into_iter().max_by(|a, b| probability(*a).total_cmp(&probability(*b))).context("no languages")?;
    Ok(whisper_rs::get_lang_str(best).context("unknown language id")?.to_string())
}

/// True for recordings with no audible sound (e.g. the microphone delivered silence).
pub fn is_silent(pcm: &[f32]) -> bool {
    pcm.chunks(480).all(|frame| {
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        rms < 0.002
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use explainit_core::markdown::plain_text;

    fn fixture(name: &str) -> Vec<f32> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
        let mut reader = hound::WavReader::open(path).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        reader.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
    }

    #[test]
    fn chooses_the_most_likely_allowed_language() {
        let id = |code: &str| whisper_rs::get_lang_id(code).unwrap() as usize;
        let mut probabilities = vec![0.0f32; whisper_rs::get_lang_max_id() as usize + 1];
        probabilities[id("it")] = 0.6;
        probabilities[id("ro")] = 0.3;
        probabilities[id("en")] = 0.1;
        let allowed = |codes: &[&str]| codes.iter().map(|c| c.to_string()).collect::<Vec<_>>();
        assert_eq!(choose_language(&probabilities, &allowed(&["en", "ro"])).unwrap(), "ro");
        assert_eq!(choose_language(&probabilities, &[]).unwrap(), "it");
        assert!(choose_language(&probabilities, &allowed(&["en", "klingon"])).is_err());
    }

    #[test]
    fn detects_silence() {
        assert!(is_silent(&vec![0.0; 16_000]));
        assert!(is_silent(&vec![0.001; 16_000]), "a faint hum is not sound");
        let speech_level: Vec<f32> = (0..16_000).map(|i| 0.1 * (i as f32 * 0.05).sin()).collect();
        assert!(!is_silent(&speech_level));
    }

    #[test]
    #[ignore = "needs the large-v3-turbo model in ~/Library/Application Support/ExplainIt/models"]
    fn transcribes_romanian_and_english_verbatim() {
        let spec = ModelSpec::find("large-v3-turbo");
        let transcriber = Transcriber::load(spec.id, &spec.path()).unwrap();

        let started = Instant::now();
        let languages = ["ro".to_string(), "en".to_string()];
        let ro = transcriber.transcribe(&fixture("ro_bug.wav"), &languages).unwrap();
        println!("ro ({:.2?}): {}", started.elapsed(), plain_text(&ro));
        assert!(plain_text(&ro).contains("Butonul de salvare nu face nimic"));
        assert!(plain_text(&ro).contains("ecrane mici"));

        let en = transcriber.transcribe(&fixture("en_bug.wav"), &[]).unwrap();
        println!("en: {}", plain_text(&en));
        assert!(plain_text(&en).contains("padding on the summary card"));
        assert!(en.windows(2).all(|w| w[0].start <= w[1].start), "segments are in order");
        assert!(en.last().unwrap().end > 9.0, "timestamps cover the clip");
    }

    #[test]
    #[ignore = "downloads 190 MB"]
    fn downloads_and_verifies_a_model() {
        let spec = ModelSpec::find("small");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(spec.file);
        let reports = std::sync::atomic::AtomicUsize::new(0);
        download(spec, &path, |_| {
            reports.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })
        .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), spec.size);
        assert!(reports.load(std::sync::atomic::Ordering::Relaxed) > 0);
    }
}
