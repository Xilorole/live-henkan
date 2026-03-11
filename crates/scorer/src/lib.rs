//! Neural language model scorer for kana-kanji re-ranking.
//!
//! Loads a character-level Transformer LM (trained in Python, exported as
//! safetensors) and scores candidate strings by perplexity. Lower perplexity
//! means more natural Japanese text.
//!
//! # Architecture
//!
//! The model is a small causal Transformer (GPT-like):
//! - Character-level tokenization (Unicode codepoints)
//! - 3 layers, 256-dim, 4 heads (~2M params)
//! - Trained on next-character prediction over Japanese Wikipedia
//!
//! # Usage
//!
//! ```ignore
//! let scorer = LMScorer::load("data/model/")?;
//! let score = scorer.score("今日はいい天気ですね");
//! // Lower score = more natural
//! ```

use std::collections::HashMap;
use std::path::Path;

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{embedding, layer_norm, linear, Activation, Module, VarBuilder};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScorerError {
    #[error("Model loading error: {0}")]
    Load(String),
    #[error("Inference error: {0}")]
    Inference(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Candle error: {0}")]
    Candle(#[from] candle_core::Error),
}

/// Model configuration (matches training/model.py LMConfig).
#[derive(Debug, Deserialize, Clone)]
pub struct LMConfig {
    pub vocab_size: usize,
    pub d_model: usize,
    pub n_head: usize,
    pub n_layer: usize,
    pub d_ff: usize,
    pub max_seq_len: usize,
    #[serde(default = "default_dropout")]
    pub dropout: f64,
}

fn default_dropout() -> f64 {
    0.0
}

/// Character-level vocabulary.
#[derive(Debug)]
pub struct CharVocab {
    char_to_id: HashMap<char, u32>,
    #[allow(dead_code)]
    pad_id: u32,
    unk_id: u32,
    bos_id: u32,
    eos_id: u32,
}

impl CharVocab {
    /// Load vocabulary from a text file (one character per line).
    pub fn load(path: &Path) -> Result<Self, ScorerError> {
        let content = std::fs::read_to_string(path)?;
        let mut char_to_id = HashMap::new();
        for (i, line) in content.lines().enumerate() {
            let ch = line.chars().next();
            if let Some(c) = ch {
                char_to_id.insert(c, i as u32);
            }
        }
        // Special token IDs (must match training/model.py)
        Ok(Self {
            char_to_id,
            pad_id: 0,
            unk_id: 1,
            bos_id: 2,
            eos_id: 3,
        })
    }

    /// Encode a string to token IDs with BOS/EOS.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut ids = vec![self.bos_id];
        for c in text.chars() {
            ids.push(*self.char_to_id.get(&c).unwrap_or(&self.unk_id));
        }
        ids.push(self.eos_id);
        ids
    }
}

/// Causal self-attention layer.
struct CausalSelfAttention {
    qkv: candle_nn::Linear,
    proj: candle_nn::Linear,
    n_head: usize,
    d_head: usize,
}

impl CausalSelfAttention {
    fn load(vb: VarBuilder, config: &LMConfig) -> Result<Self, candle_core::Error> {
        let d_head = config.d_model / config.n_head;
        Ok(Self {
            qkv: linear(config.d_model, 3 * config.d_model, vb.pp("qkv"))?,
            proj: linear(config.d_model, config.d_model, vb.pp("proj"))?,
            n_head: config.n_head,
            d_head,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, candle_core::Error> {
        let (b, t, c) = x.dims3()?;
        let qkv = self.qkv.forward(x)?;
        let q = qkv.narrow(2, 0, c)?;
        let k = qkv.narrow(2, c, c)?;
        let v = qkv.narrow(2, 2 * c, c)?;

        let q = q
            .reshape((b, t, self.n_head, self.d_head))?
            .transpose(1, 2)?;
        let k = k
            .reshape((b, t, self.n_head, self.d_head))?
            .transpose(1, 2)?;
        let v = v
            .reshape((b, t, self.n_head, self.d_head))?
            .transpose(1, 2)?;

        let scale = (self.d_head as f64).sqrt();
        let att = (q.matmul(&k.transpose(2, 3)?)? / scale)?;

        // Causal mask
        let mask = Tensor::new(
            (0..t as u32)
                .flat_map(|i| {
                    (0..t as u32).map(move |j| if j <= i { 0f32 } else { f32::NEG_INFINITY })
                })
                .collect::<Vec<_>>(),
            x.device(),
        )?
        .reshape((1, 1, t, t))?;

        let att = (att + mask)?;
        let att = candle_nn::ops::softmax(&att, candle_core::D::Minus1)?;
        let y = att.matmul(&v)?;
        let y = y.transpose(1, 2)?.reshape((b, t, c))?;
        self.proj.forward(&y)
    }
}

/// Transformer block (attention + feed-forward).
struct TransformerBlock {
    ln1: candle_nn::LayerNorm,
    attn: CausalSelfAttention,
    ln2: candle_nn::LayerNorm,
    ff1: candle_nn::Linear,
    ff2: candle_nn::Linear,
}

impl TransformerBlock {
    fn load(vb: VarBuilder, config: &LMConfig) -> Result<Self, candle_core::Error> {
        Ok(Self {
            ln1: layer_norm(
                config.d_model,
                candle_nn::LayerNormConfig::default(),
                vb.pp("ln1"),
            )?,
            attn: CausalSelfAttention::load(vb.pp("attn"), config)?,
            ln2: layer_norm(
                config.d_model,
                candle_nn::LayerNormConfig::default(),
                vb.pp("ln2"),
            )?,
            ff1: linear(config.d_model, config.d_ff, vb.pp("ff").pp("0"))?,
            ff2: linear(config.d_ff, config.d_model, vb.pp("ff").pp("2"))?,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor, candle_core::Error> {
        let residual = x;
        let x = self.ln1.forward(x)?;
        let x = self.attn.forward(&x)?;
        let x = (residual + x)?;

        let residual = &x;
        let h = self.ln2.forward(&x)?;
        let h = self.ff1.forward(&h)?;
        let h = h.apply(&Activation::Gelu)?;
        let h = self.ff2.forward(&h)?;
        residual + h
    }
}

/// Character-level causal language model.
struct CharLMModel {
    tok_emb: candle_nn::Embedding,
    pos_emb: candle_nn::Embedding,
    blocks: Vec<TransformerBlock>,
    ln_f: candle_nn::LayerNorm,
    head: candle_nn::Linear,
}

impl CharLMModel {
    fn load(vb: VarBuilder, config: &LMConfig) -> Result<Self, candle_core::Error> {
        let tok_emb = embedding(config.vocab_size, config.d_model, vb.pp("tok_emb"))?;
        let pos_emb = embedding(config.max_seq_len, config.d_model, vb.pp("pos_emb"))?;
        let mut blocks = Vec::new();
        for i in 0..config.n_layer {
            blocks.push(TransformerBlock::load(
                vb.pp(format!("blocks.{i}")),
                config,
            )?);
        }
        let ln_f = layer_norm(
            config.d_model,
            candle_nn::LayerNormConfig::default(),
            vb.pp("ln_f"),
        )?;
        let head = linear(config.d_model, config.vocab_size, vb.pp("head"))?;
        Ok(Self {
            tok_emb,
            pos_emb,
            blocks,
            ln_f,
            head,
        })
    }

    fn forward(&self, idx: &Tensor) -> Result<Tensor, candle_core::Error> {
        let (_b, t) = idx.dims2()?;
        let tok = self.tok_emb.forward(idx)?;
        let positions = Tensor::arange(0u32, t as u32, idx.device())?;
        let pos = self.pos_emb.forward(&positions)?;
        let mut x = (tok + pos.unsqueeze(0)?)?;
        for block in &self.blocks {
            x = block.forward(&x)?;
        }
        let x = self.ln_f.forward(&x)?;
        self.head.forward(&x)
    }
}

/// Neural LM scorer for N-best re-ranking.
///
/// Loads a pre-trained character-level Transformer and scores text strings
/// by negative log-likelihood per character. Lower score = more natural.
pub struct LMScorer {
    model: CharLMModel,
    vocab: CharVocab,
    device: Device,
}

impl LMScorer {
    /// Load model from a directory containing `model.safetensors`, `config.json`, `vocab.txt`.
    pub fn load(model_dir: &Path) -> Result<Self, ScorerError> {
        let device = Device::Cpu;

        // Load config
        let config_path = model_dir.join("config.json");
        let config_str = std::fs::read_to_string(&config_path)
            .map_err(|e| ScorerError::Load(format!("config.json: {e}")))?;
        let config: LMConfig = serde_json::from_str(&config_str)
            .map_err(|e| ScorerError::Load(format!("config parse: {e}")))?;

        // Load vocab
        let vocab = CharVocab::load(&model_dir.join("vocab.txt"))?;

        // Load weights
        let weights_path = model_dir.join("model.safetensors");
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)
                .map_err(|e| ScorerError::Load(format!("weights load: {e}")))?
        };
        let model = CharLMModel::load(vb, &config)?;

        Ok(Self {
            model,
            vocab,
            device,
        })
    }

    /// Score a text string by negative log-likelihood per character.
    ///
    /// Returns the average NLL — lower means the text is more natural/likely.
    /// Used to re-rank N-best conversion paths.
    pub fn score(&self, text: &str) -> Result<f32, ScorerError> {
        if text.is_empty() {
            return Ok(f32::MAX);
        }

        let ids = self.vocab.encode(text);
        let ids_tensor = Tensor::new(ids.as_slice(), &self.device)?
            .unsqueeze(0)?
            .to_dtype(DType::U32)?;

        let logits = self.model.forward(&ids_tensor)?; // (1, T, V)
        let logits = logits.i(0)?; // (T, V)

        // Compute NLL: for each position i, loss = -log P(token[i+1] | token[0..=i])
        let seq_len = ids.len();
        if seq_len < 2 {
            return Ok(f32::MAX);
        }

        let logits = logits.narrow(0, 0, seq_len - 1)?; // (T-1, V)
        let targets: Vec<u32> = ids[1..].to_vec();
        let targets_tensor = Tensor::new(targets.as_slice(), &self.device)?;

        let log_probs = candle_nn::ops::log_softmax(&logits, candle_core::D::Minus1)?;

        // Gather log probs for target tokens
        let targets_expanded = targets_tensor.unsqueeze(1)?;
        let selected = log_probs.gather(&targets_expanded, 1)?;
        let nll = selected.squeeze(1)?.neg()?;
        let mean_nll = nll.mean(0)?.to_scalar::<f32>()?;

        Ok(mean_nll)
    }

    /// Score multiple texts and return scores in the same order.
    pub fn score_batch(&self, texts: &[String]) -> Result<Vec<f32>, ScorerError> {
        texts.iter().map(|t| self.score(t)).collect()
    }

    /// Re-rank N-best paths by neural LM score.
    ///
    /// Takes paths as `(viterbi_cost, surface_text)` pairs and returns them
    /// sorted by LM score (best first). The `alpha` parameter controls the
    /// interpolation: `final_score = alpha * lm_score + (1-alpha) * normalized_viterbi`.
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
    fn test_vocab_encode() {
        // Create a minimal vocab file in memory
        let vocab_content = "<pad>\n<unk>\n<bos>\n<eos>\na\nb\nc\n";
        let dir = std::env::temp_dir().join("scorer_test_vocab");
        std::fs::create_dir_all(&dir).unwrap();
        let vocab_path = dir.join("vocab.txt");
        std::fs::write(&vocab_path, vocab_content).unwrap();

        let vocab = CharVocab::load(&vocab_path).unwrap();
        let ids = vocab.encode("abc");
        // BOS=2, a=4, b=5, c=6, EOS=3
        assert_eq!(ids, vec![2, 4, 5, 6, 3]);
    }

    #[test]
    fn test_vocab_unknown() {
        let vocab_content = "<pad>\n<unk>\n<bos>\n<eos>\na\n";
        let dir = std::env::temp_dir().join("scorer_test_unk");
        std::fs::create_dir_all(&dir).unwrap();
        let vocab_path = dir.join("vocab.txt");
        std::fs::write(&vocab_path, vocab_content).unwrap();

        let vocab = CharVocab::load(&vocab_path).unwrap();
        let ids = vocab.encode("az");
        // 'z' is unknown → UNK_ID=1
        assert_eq!(ids, vec![2, 4, 1, 3]);
    }

    #[test]
    fn test_config_deserialize() {
        let json = r#"{"vocab_size":4096,"d_model":256,"n_head":4,"n_layer":3,"d_ff":512,"max_seq_len":256}"#;
        let config: LMConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.vocab_size, 4096);
        assert_eq!(config.d_model, 256);
        assert_eq!(config.n_layer, 3);
    }
}
