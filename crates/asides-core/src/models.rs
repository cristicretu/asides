//! Whisper models (ggml, from huggingface.co/ggerganov/whisper.cpp). All are multilingual.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "large-v3-turbo",
        label: "large-v3-turbo (574 MB, recommended)",
        file: "ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size: 574_041_195,
    },
    ModelSpec {
        id: "large-v3",
        label: "large-v3 (1.1 GB, slower, most accurate)",
        file: "ggml-large-v3-q5_0.bin",
        sha256: "d75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1",
        size: 1_081_140_203,
    },
    ModelSpec {
        id: "small",
        label: "small (190 MB, fastest, least accurate)",
        file: "ggml-small-q5_1.bin",
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
        size: 190_085_487,
    },
];

impl ModelSpec {
    /// Unknown ids fall back to the default model.
    pub fn find(id: &str) -> &'static ModelSpec {
        MODELS.iter().find(|m| m.id == id).unwrap_or(&MODELS[0])
    }

    pub fn url(&self) -> String {
        format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", self.file)
    }

    pub fn path(&self) -> PathBuf {
        crate::paths::models_dir().join(self.file)
    }
}
