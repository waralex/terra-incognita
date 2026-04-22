//! ONNX Runtime embedder — loads all-MiniLM-L6-v2 for real embeddings.
//!
//! Requires the `onnx` feature flag.
//! Model and tokenizer files must be on disk (not bundled).

#[cfg(feature = "onnx")]
mod inner {
    use std::path::Path;
    use std::sync::Mutex;

    use ort::session::Session;
    use ort::value::Tensor;
    use tokenizers::Tokenizer;

    use crate::embed::Embedder;
    use crate::io::DbError;

    const DIMENSIONS: usize = 384;

    /// Embedder backed by ONNX Runtime running all-MiniLM-L6-v2.
    pub struct OnnxEmbedder {
        session: Mutex<Session>,
        tokenizer: Tokenizer,
    }

    impl OnnxEmbedder {
        /// Load from a directory containing `model.onnx` and `tokenizer.json`.
        pub fn from_dir(dir: &Path) -> Result<Self, DbError> {
            let model_path = dir.join("model.onnx");
            let tokenizer_path = dir.join("tokenizer.json");

            let session = Session::builder()
                .map_err(|e| DbError::Storage(format!("ONNX builder error: {}", e)))?
                .with_intra_threads(1)
                .map_err(|e| DbError::Storage(format!("ONNX threads error: {}", e)))?
                .commit_from_file(&model_path)
                .map_err(|e| DbError::Storage(format!("ONNX session error: {}", e)))?;

            let tokenizer = Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| DbError::Storage(format!("tokenizer error: {}", e)))?;

            Ok(Self {
                session: Mutex::new(session),
                tokenizer,
            })
        }
    }

    impl Embedder for OnnxEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>, DbError> {
            let encoding = self
                .tokenizer
                .encode(text, true)
                .map_err(|e| DbError::Storage(format!("tokenize error: {}", e)))?;

            let ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
            let attention: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|&m| m as i64)
                .collect();
            let token_types: Vec<i64> = encoding.get_type_ids().iter().map(|&t| t as i64).collect();
            let len = ids.len();

            let ids_tensor = Tensor::from_array(([1, len], ids))
                .map_err(|e| DbError::Storage(format!("tensor error: {}", e)))?;
            let mask_tensor = Tensor::from_array(([1, len], attention))
                .map_err(|e| DbError::Storage(format!("tensor error: {}", e)))?;
            let type_tensor = Tensor::from_array(([1, len], token_types))
                .map_err(|e| DbError::Storage(format!("tensor error: {}", e)))?;

            let mut session = self
                .session
                .lock()
                .map_err(|e| DbError::Storage(format!("session lock error: {}", e)))?;
            let outputs = session
                .run(ort::inputs![ids_tensor, mask_tensor, type_tensor])
                .map_err(|e| DbError::Storage(format!("ort run error: {}", e)))?;

            // Output shape: (1, tokens, 384). Extract then release the lock.
            let (shape, data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| DbError::Storage(format!("ort extract error: {}", e)))?;

            let token_count = shape[1] as usize;
            let dim = shape[2] as usize;
            let flat: Vec<f32> = data.to_vec();
            drop(outputs);
            drop(session);

            let mut pooled = vec![0.0f32; dim];
            for t in 0..token_count {
                for d in 0..dim {
                    pooled[d] += flat[t * dim + d];
                }
            }
            let count = token_count as f32;
            for v in &mut pooled {
                *v /= count;
            }

            // L2 normalize.
            let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in &mut pooled {
                    *v /= norm;
                }
            }

            Ok(pooled)
        }

        fn dimensions(&self) -> usize {
            DIMENSIONS
        }
    }
}

#[cfg(feature = "onnx")]
pub use inner::OnnxEmbedder;
