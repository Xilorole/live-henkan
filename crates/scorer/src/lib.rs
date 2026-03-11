//! Neural language model scorer for kana-kanji re-ranking.
//!
//! Uses [llama.cpp](https://github.com/ggerganov/llama.cpp) via `llama-cpp-2`
//! to load a pre-trained Japanese language model (GGUF format) and score
//! candidate strings by perplexity. Lower perplexity = more natural text.
//!
//! # Model
//!
//! Uses the [jinen](https://huggingface.co/togatogah/jinen-v1-xsmall.gguf)
//! model family from the karukan project — a GPT-2 based character-level LM
//! trained on Japanese text. The model is automatically downloaded from
//! HuggingFace Hub on first use (~20MB).
//!
//! # Usage
//!
//! ```ignore
//! let scorer = LMScorer::load_default()?;
//! let score = scorer.score("今日はいい天気ですね")?;
//! // Lower score = more natural
//! ```

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hf_hub::api::sync::ApiBuilder;
use hf_hub::{Repo, RepoType};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::token::LlamaToken;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScorerError {
    #[error("Model loading error: {0}")]
    Load(String),
    #[error("Inference error: {0}")]
    Inference(String),
    #[error("Download error: {0}")]
    Download(String),
    #[error("Tokenizer error: {0}")]
    Tokenizer(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Default HuggingFace repository for the jinen xsmall model.
const DEFAULT_REPO_ID: &str = "togatogah/jinen-v1-xsmall.gguf";
/// Default GGUF filename (Q5_K_M quantization, ~20MB).
const DEFAULT_GGUF_FILENAME: &str = "jinen-v1-xsmall-Q5_K_M.gguf";
/// Default context window size.
const DEFAULT_N_CTX: u32 = 256;

/// Global llama.cpp backend (can only be initialized once).
static LLAMA_BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();

fn get_backend() -> Result<&'static LlamaBackend, ScorerError> {
    let result = LLAMA_BACKEND.get_or_init(|| {
        let mut backend = LlamaBackend::init().map_err(|e| e.to_string())?;
        backend.void_logs();
        Ok(backend)
    });
    match result {
        Ok(backend) => Ok(backend),
        Err(e) => Err(ScorerError::Load(format!(
            "Failed to initialize llama.cpp backend: {e}"
        ))),
    }
}

/// Download a file from HuggingFace Hub, caching locally.
fn download_hf_file(repo_id: &str, filename: &str) -> Result<PathBuf, ScorerError> {
    let mut builder = ApiBuilder::new();
    if let Ok(token) = std::env::var("HF_TOKEN") {
        builder = builder.with_token(Some(token));
    }
    let api = builder
        .build()
        .map_err(|e| ScorerError::Download(format!("HF API init: {e}")))?;
    let repo = api.repo(Repo::new(repo_id.to_string(), RepoType::Model));
    let path = repo
        .get(filename)
        .map_err(|e| ScorerError::Download(format!("{repo_id}/{filename}: {e}")))?;
    Ok(path)
}

/// Load an external HuggingFace tokenizer from `tokenizer.json`.
fn load_tokenizer(path: &Path) -> Result<tokenizers::Tokenizer, ScorerError> {
    let mut tokenizer = tokenizers::Tokenizer::from_file(path)
        .map_err(|e| ScorerError::Tokenizer(format!("{e}")))?;
    tokenizer.with_padding(None);
    tokenizer.with_truncation(None).ok();
    Ok(tokenizer)
}

/// Neural LM scorer for N-best re-ranking.
///
/// Wraps a llama.cpp GGUF model and scores text by negative log-likelihood
/// per character. Lower score = more natural Japanese text.
pub struct LMScorer {
    model: LlamaModel,
    tokenizer: tokenizers::Tokenizer,
    n_ctx: u32,
}

impl LMScorer {
    /// Load model from explicit local file paths.
    pub fn load(model_path: &Path, tokenizer_path: &Path) -> Result<Self, ScorerError> {
        let backend = get_backend()?;

        // GPT-2 has Metal issues on macOS, use CPU only
        let model_params = LlamaModelParams::default().with_n_gpu_layers(0);
        let model = LlamaModel::load_from_file(backend, model_path, &model_params)
            .map_err(|e| ScorerError::Load(format!("GGUF load: {e}")))?;

        let tokenizer = load_tokenizer(tokenizer_path)?;

        Ok(Self {
            model,
            tokenizer,
            n_ctx: DEFAULT_N_CTX,
        })
    }

    /// Load the default jinen-v1-xsmall model, downloading from HuggingFace if needed.
    pub fn load_default() -> Result<Self, ScorerError> {
        Self::load_from_repo(DEFAULT_REPO_ID, DEFAULT_GGUF_FILENAME)
    }

    /// Load a model from a HuggingFace repository.
    pub fn load_from_repo(repo_id: &str, gguf_filename: &str) -> Result<Self, ScorerError> {
        let model_path = download_hf_file(repo_id, gguf_filename)?;
        let tokenizer_path = download_hf_file(repo_id, "tokenizer.json")?;
        Self::load(&model_path, &tokenizer_path)
    }

    /// Tokenize text using the external tokenizer.
    fn tokenize(&self, text: &str) -> Result<Vec<LlamaToken>, ScorerError> {
        let encoding = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| ScorerError::Tokenizer(format!("{e}")))?;
        Ok(encoding
            .get_ids()
            .iter()
            .map(|&id| LlamaToken(id as i32))
            .collect())
    }

    /// Score a text string by negative log-likelihood per character.
    ///
    /// Returns the average NLL — lower means the text is more natural/likely.
    /// Used to re-rank N-best conversion paths.
    pub fn score(&self, text: &str) -> Result<f32, ScorerError> {
        if text.is_empty() {
            return Ok(f32::MAX);
        }

        let tokens = self.tokenize(text)?;
        if tokens.len() < 2 {
            return Ok(f32::MAX);
        }

        let n_tokens = tokens.len();
        let backend = get_backend()?;

        let ctx_params = LlamaContextParams::default().with_n_ctx(Some(
            NonZeroU32::new(self.n_ctx).expect("n_ctx must be non-zero"),
        ));
        let mut ctx = self
            .model
            .new_context(backend, ctx_params)
            .map_err(|e| ScorerError::Inference(format!("context: {e}")))?;

        // Feed all tokens, requesting logits at every position
        let mut batch = LlamaBatch::new(n_tokens.max(512), 1);
        batch
            .add_sequence(&tokens, 0, true)
            .map_err(|e| ScorerError::Inference(format!("batch: {e}")))?;

        ctx.decode(&mut batch)
            .map_err(|e| ScorerError::Inference(format!("decode: {e}")))?;

        let vocab_size = self.model.n_vocab() as usize;

        // Compute NLL: for each position, -log P(next_token | prefix)
        let mut total_nll: f32 = 0.0;
        let mut n_scored = 0;

        for pos in 0..(n_tokens - 1) {
            let logits = ctx.get_logits_ith(pos as i32);

            // Log-softmax: log P(token) = logit - log(sum(exp(logits)))
            let max_logit = logits
                .iter()
                .take(vocab_size)
                .cloned()
                .fold(f32::NEG_INFINITY, f32::max);
            let log_sum_exp: f32 = logits
                .iter()
                .take(vocab_size)
                .map(|&x| (x - max_logit).exp())
                .sum::<f32>()
                .ln()
                + max_logit;

            let target = tokens[pos + 1].0 as usize;
            if target < vocab_size {
                total_nll -= logits[target] - log_sum_exp;
            }
            n_scored += 1;
        }

        if n_scored == 0 {
            return Ok(f32::MAX);
        }

        // Normalize by character count (not token count) for fair comparison
        // across paths with different segmentations
        let n_chars = text.chars().count().max(1);
        Ok(total_nll / n_chars as f32)
    }

    /// Re-rank N-best paths by neural LM score.
    ///
    /// Takes paths as `(viterbi_cost, surface_text)` pairs and returns them
    /// sorted by combined score (best first). The `alpha` parameter controls
    /// interpolation: `final = alpha * lm_score + (1-alpha) * normalized_viterbi`.
    pub fn rerank(
        &self,
        paths: &[(i64, String)],
        alpha: f32,
    ) -> Result<Vec<(f32, usize)>, ScorerError> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }

        // Normalize Viterbi costs to [0, 1] range
        let min_cost = paths.iter().map(|(c, _)| *c).min().unwrap() as f32;
        let max_cost = paths.iter().map(|(c, _)| *c).max().unwrap() as f32;
        let cost_range = (max_cost - min_cost).max(1.0);

        let mut scored: Vec<(f32, usize)> = Vec::with_capacity(paths.len());
        for (i, (cost, text)) in paths.iter().enumerate() {
            let lm_score = self.score(text)?;
            let norm_cost = (*cost as f32 - min_cost) / cost_range;
            let final_score = alpha * lm_score + (1.0 - alpha) * norm_cost;
            scored.push((final_score, i));
        }

        scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scorer_error_display() {
        let err = ScorerError::Load("test error".to_string());
        assert_eq!(err.to_string(), "Model loading error: test error");

        let err = ScorerError::Download("not found".to_string());
        assert_eq!(err.to_string(), "Download error: not found");
    }

    #[test]
    fn test_download_hf_file_invalid_repo() {
        let result = download_hf_file("nonexistent-user-xyz/nonexistent-repo-12345", "f.bin");
        assert!(result.is_err());
    }
}
