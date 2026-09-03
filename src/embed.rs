//! Embedding providers behind one trait.
//!
//! [`FastembedProvider`] runs a tiny ONNX model in-process (default path,
//! no server needed). [`HttpProvider`] speaks OpenAI-compatible
//! `/v1/embeddings` for a future MLX embedding lane; the current
//! `mlx-lm.server` lanes have no such endpoint (verified 404).

use std::sync::Mutex;

use crate::Error;

/// Maps texts to dense vectors. Implementations must be thread-safe.
pub trait EmbedProvider: Send + Sync {
    /// Embed each text, preserving order.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Embed`] when the backend fails.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, Error>;

    /// Vector dimensionality.
    fn dims(&self) -> usize;

    /// Stable model label recorded in the vector store.
    fn name(&self) -> &str;

    /// Query-side prefix (documents are embedded raw). Snowflake arctic
    /// models expect the BGE-style retrieval prefix; others use none.
    fn query_prefix(&self) -> Option<&str> {
        None
    }
}

/// In-process ONNX embeddings (fastembed).
pub struct FastembedProvider {
    model: Mutex<fastembed::TextEmbedding>,
    which: OnnxModel,
    name: String,
    dims: usize,
}

/// Selectable ONNX embedding model. Ordered by quality/cost trade-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnnxModel {
    /// `all-MiniLM-L6-v2`, 22M params, 384 dims. Fastest, weakest.
    #[default]
    MiniLM,
    /// `snowflake-arctic-embed-m`, 109M params, 768 dims. Better
    /// separation, ~5x compute. Apache-2.0.
    ArcticM,
    /// `embeddinggemma-300m`, 300M params, 768 dims. Best retrieval
    /// under 500M params, slowest CPU per-query. Gemma license.
    Gemma300M,
}

impl OnnxModel {
    fn embedding_model(self) -> fastembed::EmbeddingModel {
        match self {
            Self::MiniLM => fastembed::EmbeddingModel::AllMiniLML6V2,
            Self::ArcticM => fastembed::EmbeddingModel::SnowflakeArcticEmbedM,
            Self::Gemma300M => fastembed::EmbeddingModel::EmbeddingGemma300M,
        }
    }

    /// Parse a CLI label.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for unknown labels.
    pub fn parse(label: &str) -> Result<Self, Error> {
        match label {
            "minilm" => Ok(Self::MiniLM),
            "arctic-m" => Ok(Self::ArcticM),
            "gemma-300m" => Ok(Self::Gemma300M),
            other => Err(Error::InvalidInput(format!(
                "unknown embedding model: {other} (minilm|arctic-m|gemma-300m)"
            ))),
        }
    }

    /// Parse a stored provider name back, for `--hybrid` auto-detect.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Embed`] when the store was built by an unknown model.
    pub fn parse_stored(name: &str) -> Result<Self, Error> {
        match name {
            "fastembed/all-MiniLM-L6-v2" => Ok(Self::MiniLM),
            "fastembed/snowflake-arctic-embed-m" => Ok(Self::ArcticM),
            "fastembed/embeddinggemma-300m" => Ok(Self::Gemma300M),
            other => Err(Error::Embed(format!(
                "vector store uses unknown model {other}; re-run `embed`"
            ))),
        }
    }
}

impl FastembedProvider {
    /// Load the default model, downloading to the global cache on first use.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Embed`] when the model cannot be fetched or started.
    pub fn load() -> Result<Self, Error> {
        Self::load_model(OnnxModel::default())
    }

    /// Load a specific model.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Embed`] when the model cannot be fetched or started.
    pub fn load_model(which: OnnxModel) -> Result<Self, Error> {
        let cache = global_cache_dir();
        std::fs::create_dir_all(&cache).map_err(|e| Error::Embed(e.to_string()))?;
        let embedding_model = which.embedding_model();
        let dims = fastembed::TextEmbedding::get_model_info(&embedding_model)
            .map_err(|e| Error::Embed(e.to_string()))?
            .dim;
        let model = fastembed::TextEmbedding::try_new(
            fastembed::TextInitOptions::new(embedding_model)
                .with_cache_dir(cache)
                .with_show_download_progress(true),
        )
        .map_err(|e| Error::Embed(e.to_string()))?;
        Ok(Self {
            model: Mutex::new(model),
            which,
            name: format!("fastembed/{}", model_label(which)),
            dims,
        })
    }
}

fn model_label(which: OnnxModel) -> &'static str {
    match which {
        OnnxModel::MiniLM => "all-MiniLM-L6-v2",
        OnnxModel::ArcticM => "snowflake-arctic-embed-m",
        OnnxModel::Gemma300M => "embeddinggemma-300m",
    }
}

impl EmbedProvider for FastembedProvider {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, Error> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut model = self.model.lock().map_err(|e| Error::Embed(e.to_string()))?;
        model
            .embed(texts, None)
            .map_err(|e| Error::Embed(e.to_string()))
    }

    fn dims(&self) -> usize {
        self.dims
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn query_prefix(&self) -> Option<&str> {
        match self.which {
            OnnxModel::ArcticM => Some("Represent this sentence for searching relevant passages: "),
            OnnxModel::MiniLM | OnnxModel::Gemma300M => None,
        }
    }
}

/// OpenAI-compatible HTTP embeddings (`POST {base}/v1/embeddings`).
/// Placeholder for a future MLX embedding lane.
pub struct HttpProvider {
    base_url: String,
    model: String,
    dims: usize,
    client: reqwest::blocking::Client,
}

impl HttpProvider {
    /// Point at `base_url` (e.g. `http://127.0.0.1:8081`) with `model` id.
    #[must_use]
    pub fn new(base_url: &str, model: &str, dims: usize) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            model: model.to_owned(),
            dims,
            client: reqwest::blocking::Client::new(),
        }
    }
}

#[derive(serde::Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [&'a str],
}

#[derive(serde::Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDatum>,
}

#[derive(serde::Deserialize)]
struct EmbedDatum {
    embedding: Vec<f32>,
}

impl EmbedProvider for HttpProvider {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, Error> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let response: EmbedResponse = self
            .client
            .post(format!("{}/v1/embeddings", self.base_url))
            .json(&EmbedRequest {
                model: &self.model,
                input: texts,
            })
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json())
            .map_err(|e| Error::Embed(e.to_string()))?;
        response.data.into_iter().map(|d| Ok(d.embedding)).collect()
    }

    fn dims(&self) -> usize {
        self.dims
    }

    fn name(&self) -> &str {
        &self.model
    }
}

fn global_cache_dir() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(
        || std::path::PathBuf::from(".one-grep-cache"),
        |home| {
            std::path::PathBuf::from(home)
                .join(".one-grep")
                .join("models")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_labels_roundtrip() {
        for (label, model) in [
            ("minilm", OnnxModel::MiniLM),
            ("arctic-m", OnnxModel::ArcticM),
            ("gemma-300m", OnnxModel::Gemma300M),
        ] {
            assert_eq!(OnnxModel::parse(label).expect("parse"), model);
        }
        assert!(OnnxModel::parse("bert").is_err());
    }
}

/// Cross-encoder reranking over fused candidates.
pub trait Rerank: Send + Sync {
    /// Score `docs` against `query`, best first as `(doc_index, score)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Embed`] when the backend fails.
    fn rerank(&self, query: &str, docs: &[&str]) -> Result<Vec<(usize, f32)>, Error>;
}

/// In-process cross-encoder (`jina-reranker-v1-turbo-en`, ~38M params).
pub struct JinaReranker {
    model: Mutex<fastembed::TextRerank>,
}

impl JinaReranker {
    /// Load the model, downloading to the global cache on first use.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Embed`] when the model cannot be fetched or started.
    pub fn load() -> Result<Self, Error> {
        let cache = global_cache_dir();
        std::fs::create_dir_all(&cache).map_err(|e| Error::Embed(e.to_string()))?;
        let model = fastembed::TextRerank::try_new(
            fastembed::RerankInitOptions::new(fastembed::RerankerModel::JINARerankerV1TurboEn)
                .with_cache_dir(cache)
                .with_show_download_progress(true),
        )
        .map_err(|e| Error::Embed(e.to_string()))?;
        Ok(Self {
            model: Mutex::new(model),
        })
    }
}

impl Rerank for JinaReranker {
    fn rerank(&self, query: &str, docs: &[&str]) -> Result<Vec<(usize, f32)>, Error> {
        if docs.is_empty() {
            return Ok(Vec::new());
        }
        let mut model = self.model.lock().map_err(|e| Error::Embed(e.to_string()))?;
        Ok(model
            .rerank(query, docs, false, None)
            .map_err(|e| Error::Embed(e.to_string()))?
            .into_iter()
            .map(|r| (r.index, r.score))
            .collect())
    }
}
