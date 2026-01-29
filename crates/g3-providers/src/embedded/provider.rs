//! Embedded LLM provider using llama.cpp with Metal acceleration on macOS.
//!
//! Supports multiple model families with their native chat templates:
//! - Qwen (ChatML format)
//! - GLM-4 (ChatGLM4 format)
//! - Mistral (Instruct format)
//! - Llama/CodeLlama (Llama2 format)

use crate::{
    CompletionRequest, CompletionResponse, CompletionStream, LLMProvider, Message, MessageRole,
    Usage,
    streaming::{make_final_chunk_with_reason, make_text_chunk},
};
use anyhow::Result;
use llama_cpp_2::{
    context::LlamaContext,
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel, Special, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error};

// ============================================================================
// Global Backend
// ============================================================================

/// Global llama.cpp backend - can only be initialized once per process
static LLAMA_BACKEND: OnceLock<Arc<LlamaBackend>> = OnceLock::new();

/// Get or initialize the global llama.cpp backend
fn get_or_init_backend() -> Result<Arc<LlamaBackend>> {
    if let Some(backend) = LLAMA_BACKEND.get() {
        return Ok(Arc::clone(backend));
    }

    // Suppress llama.cpp's verbose logging to stderr
    suppress_llama_logging();

    debug!("Initializing llama.cpp backend...");
    let backend = LlamaBackend::init()
        .map_err(|e| anyhow::anyhow!("Failed to initialize llama.cpp backend: {:?}", e))?;

    // Store it (ignore if another thread beat us to it)
    let _ = LLAMA_BACKEND.set(Arc::new(backend));
    Ok(Arc::clone(LLAMA_BACKEND.get().expect("backend was just set")))
}

fn suppress_llama_logging() {
    unsafe {
        unsafe extern "C" fn void_log(
            _level: std::ffi::c_int,
            _text: *const std::os::raw::c_char,
            _user_data: *mut std::os::raw::c_void,
        ) {
            // Intentionally empty
        }
        extern "C" {
            fn llama_log_set(
                log_callback: Option<
                    unsafe extern "C" fn(
                        std::ffi::c_int,
                        *const std::os::raw::c_char,
                        *mut std::os::raw::c_void,
                    ),
                >,
                user_data: *mut std::os::raw::c_void,
            );
        }
        llama_log_set(Some(void_log), std::ptr::null_mut());
    }
}

// ============================================================================
// Provider Struct
// ============================================================================

use super::adapters::create_adapter_for_model;

pub struct EmbeddedProvider {
    name: String,
    model: Arc<LlamaModel>,
    backend: Arc<LlamaBackend>,
    model_type: String,
    model_name: String,
    max_tokens: Option<u32>,
    temperature: f32,
    context_length: u32,
    threads: Option<u32>,
}

impl EmbeddedProvider {
    /// Create a new embedded provider with default naming ("embedded").
    pub fn new(
        model_path: String,
        model_type: String,
        context_length: Option<u32>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        gpu_layers: Option<u32>,
        threads: Option<u32>,
    ) -> Result<Self> {
        Self::new_with_name(
            "embedded".to_string(),
            model_path,
            model_type,
            context_length,
            max_tokens,
            temperature,
            gpu_layers,
            threads,
        )
    }

    /// Create a new embedded provider with a custom name.
    pub fn new_with_name(
        name: String,
        model_path: String,
        model_type: String,
        context_length: Option<u32>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        gpu_layers: Option<u32>,
        threads: Option<u32>,
    ) -> Result<Self> {
        debug!("Loading embedded model from: {}", model_path);

        let expanded_path = shellexpand::tilde(&model_path);
        let model_path_buf = PathBuf::from(expanded_path.as_ref());

        if !model_path_buf.exists() {
            anyhow::bail!("Model file not found: {}", model_path_buf.display());
        }

        let backend = get_or_init_backend()?;

        let n_gpu_layers = gpu_layers.unwrap_or(99);
        let model_params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
        debug!("Using {} GPU layers", n_gpu_layers);

        debug!("Loading model...");
        let model = LlamaModel::load_from_file(&backend, &model_path_buf, &model_params)
            .map_err(|e| anyhow::anyhow!("Failed to load model: {:?}", e))?;

        let model_ctx_train = model.n_ctx_train();
        let context_size = context_length.unwrap_or(model_ctx_train);
        debug!(
            "Context length: {} (model trained: {}, configured: {:?})",
            context_size, model_ctx_train, context_length
        );

        debug!("Successfully loaded {} model as '{}'", model_type, name);

        Ok(Self {
            name,
            model: Arc::new(model),
            backend,
            model_type: model_type.to_lowercase(),
            model_name: format!("embedded-{}", model_type),
            max_tokens,
            temperature: temperature.unwrap_or(0.1),
            context_length: context_size,
            threads,
        })
    }

    fn effective_max_tokens(&self) -> u32 {
        self.max_tokens
            .unwrap_or_else(|| std::cmp::min(4096, self.context_length / 4))
    }

    /// Estimate token count from text (~4 chars per token)
    fn estimate_tokens(&self, text: &str) -> u32 {
        (text.len() as f32 / 4.0).ceil() as u32
    }
}

// ============================================================================
// Chat Template Formatting
// ============================================================================

impl EmbeddedProvider {
    /// Format messages according to the model's native chat template.
    fn format_messages(&self, messages: &[Message]) -> String {
        match self.model_type.as_str() {
            t if t.contains("glm") => format_glm4(messages),
            t if t.contains("qwen") => format_qwen(messages),
            t if t.contains("mistral") => format_mistral(messages),
            _ => format_llama(messages),
        }
    }

    /// Get stop sequences based on model type.
    fn get_stop_sequences(&self) -> &'static [&'static str] {
        get_stop_sequences_for_model(&self.model_type)
    }
}

/// GLM-4 ChatGLM4 format: [gMASK]<sop><|role|>\ncontent
fn format_glm4(messages: &[Message]) -> String {
    let mut out = String::from("[gMASK]<sop>");
    for msg in messages {
        let role = match msg.role {
            MessageRole::System => "<|system|>",
            MessageRole::User => "<|user|>",
            MessageRole::Assistant => "<|assistant|>",
        };
        out.push_str(&format!("{}\n{}", role, msg.content));
    }
    out.push_str("<|assistant|>\n");
    out
}

/// Qwen ChatML format: <|im_start|>role\ncontent<|im_end|>
fn format_qwen(messages: &[Message]) -> String {
    let mut out = String::new();
    for msg in messages {
        let role = match msg.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
        };
        out.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", role, msg.content));
    }
    out.push_str("<|im_start|>assistant\n");
    out
}

/// Mistral Instruct format: <s>[INST] ... [/INST] response</s>
fn format_mistral(messages: &[Message]) -> String {
    let mut out = String::new();
    let mut in_inst = false;

    for (i, msg) in messages.iter().enumerate() {
        match msg.role {
            MessageRole::System if i == 0 => {
                out.push_str("<s>[INST] ");
                out.push_str(&msg.content);
                out.push_str("\n\n");
                in_inst = true;
            }
            MessageRole::System => {} // Ignore non-first system messages
            MessageRole::User => {
                if !in_inst {
                    out.push_str("<s>[INST] ");
                }
                out.push_str(&msg.content);
                out.push_str(" [/INST]");
                in_inst = false;
            }
            MessageRole::Assistant => {
                out.push(' ');
                out.push_str(&msg.content);
                out.push_str("</s> ");
                in_inst = false;
            }
        }
    }

    if messages.last().is_some_and(|m| matches!(m.role, MessageRole::User)) {
        out.push(' ');
    }
    out
}

/// Llama/CodeLlama format: [INST] <<SYS>>\nsystem<</SYS>>\n\nuser [/INST]
fn format_llama(messages: &[Message]) -> String {
    let mut out = String::new();
    for msg in messages {
        match msg.role {
            MessageRole::System => {
                out.push_str(&format!("[INST] <<SYS>>\n{}\n<</SYS>>\n\n", msg.content));
            }
            MessageRole::User => {
                out.push_str(&format!("{} [/INST] ", msg.content));
            }
            MessageRole::Assistant => {
                out.push_str(&format!("{} </s><s>[INST] ", msg.content));
            }
        }
    }
    out
}

/// Get stop sequences for a model type.
fn get_stop_sequences_for_model(model_type: &str) -> &'static [&'static str] {
    if model_type.contains("glm") {
        &["<|endoftext|>", "<|user|>", "<|observation|>", "<|system|>"]
    } else if model_type.contains("qwen") {
        &["<|im_end|>", "<|endoftext|>", "</s>", "<|im_start|>"]
    } else if model_type.contains("code-llama") || model_type.contains("codellama") {
        &["</s>", "[/INST]", "<</SYS>>", "[INST]", "<<SYS>>"]
    } else if model_type.contains("llama") {
        &[
            "</s>",
            "[/INST]",
            "<</SYS>>",
            "### Human:",
            "### Assistant:",
            "[INST]",
        ]
    } else if model_type.contains("mistral") {
        &["</s>", "[/INST]", "<|im_end|>"]
    } else if model_type.contains("vicuna") || model_type.contains("wizard") {
        &[
            "### Human:",
            "### Assistant:",
            "USER:",
            "ASSISTANT:",
            "</s>",
        ]
    } else if model_type.contains("alpaca") {
        &["### Instruction:", "### Response:", "### Input:", "</s>"]
    } else {
        // Generic fallback
        &[
            "</s>",
            "<|endoftext|>",
            "<|im_end|>",
            "### Human:",
            "### Assistant:",
            "[/INST]",
            "<</SYS>>",
        ]
    }
}

// ============================================================================
// Inference Helpers
// ============================================================================

/// Parameters for inference, extracted from request and provider defaults.
struct InferenceParams {
    prompt: String,
    max_tokens: u32,
    temperature: f32,
    stop_sequences: Vec<String>,
}

/// Prepared inference context with tokenized prompt ready for generation.
struct PreparedContext<'a> {
    ctx: LlamaContext<'a>,
    batch: LlamaBatch,
    sampler: LlamaSampler,
    token_count: i32,
}

impl EmbeddedProvider {
    /// Extract inference parameters from a completion request.
    fn extract_params(&self, request: &CompletionRequest) -> InferenceParams {
        InferenceParams {
            prompt: self.format_messages(&request.messages),
            max_tokens: request.max_tokens.unwrap_or_else(|| self.effective_max_tokens()),
            temperature: request.temperature.unwrap_or(self.temperature),
            stop_sequences: self
                .get_stop_sequences()
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

/// Prepare the inference context: create context, tokenize prompt, decode initial batch.
fn prepare_context<'a>(
    model: &'a LlamaModel,
    backend: &'a LlamaBackend,
    prompt: &str,
    temperature: f32,
    context_length: u32,
    threads: Option<u32>,
) -> Result<PreparedContext<'a>> {
    let n_ctx = NonZeroU32::new(context_length).unwrap_or(NonZeroU32::new(4096).unwrap());
    let mut ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_n_batch(context_length);
    if let Some(n_threads) = threads {
        ctx_params = ctx_params.with_n_threads(n_threads as i32);
    }

    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| anyhow::anyhow!("Failed to create context: {:?}", e))?;

    let tokens = model
        .str_to_token(prompt, AddBos::Always)
        .map_err(|e| anyhow::anyhow!("Failed to tokenize: {:?}", e))?;

    debug!("Tokenized prompt: {} tokens", tokens.len());

    let batch_size = std::cmp::max(512, tokens.len());
    let mut batch = LlamaBatch::new(batch_size, 1);
    for (i, token) in tokens.iter().enumerate() {
        batch
            .add(*token, i as i32, &[0], i == tokens.len() - 1)
            .map_err(|e| anyhow::anyhow!("Failed to add token to batch: {:?}", e))?;
    }

    ctx.decode(&mut batch)
        .map_err(|e| anyhow::anyhow!("Failed to decode prompt: {:?}", e))?;

    let sampler = LlamaSampler::chain_simple([
        LlamaSampler::temp(temperature),
        LlamaSampler::dist(1234),
    ]);

    Ok(PreparedContext {
        ctx,
        batch,
        sampler,
        token_count: tokens.len() as i32,
    })
}

/// Check if text contains any stop sequence. Returns the truncation position if found.
fn find_stop_sequence(text: &str, stop_sequences: &[String]) -> Option<usize> {
    for stop_seq in stop_sequences {
        if let Some(pos) = text.find(stop_seq) {
            return Some(pos);
        }
    }
    None
}

/// Truncate text at the first stop sequence, if any.
fn truncate_at_stop_sequence(text: &mut String, stop_sequences: &[String]) {
    if let Some(pos) = find_stop_sequence(text, stop_sequences) {
        text.truncate(pos);
    }
}

// ============================================================================
// LLMProvider Implementation
// ============================================================================

#[async_trait::async_trait]
impl LLMProvider for EmbeddedProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        debug!(
            "Processing completion request with {} messages",
            request.messages.len()
        );

        let params = self.extract_params(&request);
        let prompt_tokens = self.estimate_tokens(&params.prompt);

        debug!("Formatted prompt length: {} chars", params.prompt.len());

        // Clone what we need for the blocking task
        let model = self.model.clone();
        let backend = self.backend.clone();
        let context_length = self.context_length;
        let threads = self.threads;
        let model_name = self.model_name.clone();

        let (content, completion_tokens) = tokio::task::spawn_blocking(move || {
            let mut prepared = prepare_context(
                &model,
                &backend,
                &params.prompt,
                params.temperature,
                context_length,
                threads,
            )?;

            let mut generated_text = String::new();
            let mut token_count = 0u32;

            for _ in 0..params.max_tokens {
                let new_token = prepared.sampler.sample(&prepared.ctx, prepared.batch.n_tokens() - 1);
                prepared.sampler.accept(new_token);

                if model.is_eog_token(new_token) {
                    debug!("Hit end-of-generation token at {} tokens", token_count);
                    break;
                }

                let token_str = model
                    .token_to_str(new_token, Special::Tokenize)
                    .unwrap_or_default();
                generated_text.push_str(&token_str);
                token_count += 1;

                if find_stop_sequence(&generated_text, &params.stop_sequences).is_some() {
                    debug!("Hit stop sequence at {} tokens", token_count);
                    break;
                }

                // Prepare next batch
                prepared.batch.clear();
                prepared
                    .batch
                    .add(new_token, prepared.token_count, &[0], true)
                    .map_err(|e| anyhow::anyhow!("Failed to add token to batch: {:?}", e))?;
                prepared.token_count += 1;

                prepared
                    .ctx
                    .decode(&mut prepared.batch)
                    .map_err(|e| anyhow::anyhow!("Failed to decode: {:?}", e))?;
            }

            truncate_at_stop_sequence(&mut generated_text, &params.stop_sequences);

            Ok::<_, anyhow::Error>((generated_text.trim().to_string(), token_count))
        })
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;

        Ok(CompletionResponse {
            content,
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
            model: model_name,
        })
    }

    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream> {
        debug!(
            "Processing streaming request with {} messages",
            request.messages.len()
        );

        let params = self.extract_params(&request);
        let prompt_tokens = self.estimate_tokens(&params.prompt);

        let (tx, rx) = mpsc::channel(100);

        let model = self.model.clone();
        let backend = self.backend.clone();
        let context_length = self.context_length;
        let threads = self.threads;
        let model_type = self.model_type.clone();

        tokio::task::spawn_blocking(move || {
            // Create adapter for model-specific tool format transformation (e.g., GLM)
            let mut adapter = create_adapter_for_model(&model_type);

            let mut prepared = match prepare_context(
                &model,
                &backend,
                &params.prompt,
                params.temperature,
                context_length,
                threads,
            ) {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.blocking_send(Err(e));
                    return;
                }
            };

            let mut accumulated_text = String::new();
            let mut token_count = 0u32;
            let mut stop_reason: Option<String> = None;

            for _ in 0..params.max_tokens {
                let new_token = prepared.sampler.sample(&prepared.ctx, prepared.batch.n_tokens() - 1);
                prepared.sampler.accept(new_token);

                if model.is_eog_token(new_token) {
                    debug!("Hit end-of-generation token at {} tokens", token_count);
                    stop_reason = Some("end_turn".to_string());
                    break;
                }

                let token_str = model
                    .token_to_str(new_token, Special::Tokenize)
                    .unwrap_or_default();

                accumulated_text.push_str(&token_str);
                token_count += 1;

                if find_stop_sequence(&accumulated_text, &params.stop_sequences).is_some() {
                    debug!("Hit stop sequence at {} tokens", token_count);
                    stop_reason = Some("stop_sequence".to_string());
                    break;
                }

                // Stream the token (through adapter if present)
                let output_text = if let Some(ref mut adapt) = adapter {
                    let output = adapt.process_chunk(&token_str);
                    output.emit
                } else {
                    token_str
                };
                if !output_text.is_empty() {
                    if tx.blocking_send(Ok(make_text_chunk(output_text))).is_err() {
                        return; // Receiver dropped
                    }
                }

                if token_count >= params.max_tokens {
                    debug!("Reached max token limit: {}", params.max_tokens);
                    stop_reason = Some("max_tokens".to_string());
                    break;
                }

                // Prepare next batch
                prepared.batch.clear();
                if let Err(e) = prepared.batch.add(new_token, prepared.token_count, &[0], true) {
                    error!("Failed to add token to batch: {:?}", e);
                    break;
                }
                prepared.token_count += 1;

                if let Err(e) = prepared.ctx.decode(&mut prepared.batch) {
                    error!("Failed to decode: {:?}", e);
                    break;
                }
            }

            // Flush any remaining content from the adapter
            if let Some(ref mut adapt) = adapter {
                let final_output = adapt.flush();
                if !final_output.emit.is_empty() {
                    if tx.blocking_send(Ok(make_text_chunk(final_output.emit))).is_err() {
                        return;
                    }
                }
            }

            let usage = Usage {
                prompt_tokens,
                completion_tokens: token_count,
                total_tokens: prompt_tokens + token_count,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            };
            let final_chunk =
                make_final_chunk_with_reason(vec![], Some(usage), stop_reason.or(Some("end_turn".to_string())));
            let _ = tx.blocking_send(Ok(final_chunk));
        });

        Ok(ReceiverStream::new(rx))
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn model(&self) -> &str {
        &self.model_name
    }

    fn max_tokens(&self) -> u32 {
        self.effective_max_tokens()
    }

    fn temperature(&self) -> f32 {
        self.temperature
    }

    fn context_window_size(&self) -> Option<u32> {
        Some(self.context_length)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_glm4_messages() {
        let messages = vec![
            Message::new(MessageRole::System, "You are a helpful assistant.".to_string()),
            Message::new(MessageRole::User, "Hello!".to_string()),
        ];

        let formatted = format_glm4(&messages);

        assert!(formatted.starts_with("[gMASK]<sop>"));
        assert!(formatted.contains("<|system|>\nYou are a helpful assistant."));
        assert!(formatted.contains("<|user|>\nHello!"));
        assert!(formatted.ends_with("<|assistant|>\n"));
    }

    #[test]
    fn test_format_qwen_messages() {
        let messages = vec![
            Message::new(MessageRole::System, "You are a helpful assistant.".to_string()),
            Message::new(MessageRole::User, "Hello!".to_string()),
        ];

        let formatted = format_qwen(&messages);

        assert!(formatted.contains("<|im_start|>system\nYou are a helpful assistant.<|im_end|>"));
        assert!(formatted.contains("<|im_start|>user\nHello!<|im_end|>"));
        assert!(formatted.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn test_format_mistral_messages() {
        let messages = vec![
            Message::new(MessageRole::System, "You are a helpful assistant.".to_string()),
            Message::new(MessageRole::User, "Hello!".to_string()),
        ];

        let formatted = format_mistral(&messages);

        assert!(formatted.starts_with("<s>[INST] "));
        assert!(formatted.contains("You are a helpful assistant."));
        assert!(formatted.contains("Hello!"));
        assert!(formatted.contains("[/INST]"));
    }

    #[test]
    fn test_format_llama_messages() {
        let messages = vec![
            Message::new(MessageRole::System, "You are a helpful assistant.".to_string()),
            Message::new(MessageRole::User, "Hello!".to_string()),
        ];

        let formatted = format_llama(&messages);

        assert!(formatted.contains("<<SYS>>"));
        assert!(formatted.contains("You are a helpful assistant."));
        assert!(formatted.contains("<</SYS>>"));
        assert!(formatted.contains("Hello!"));
        assert!(formatted.contains("[/INST]"));
    }

    #[test]
    fn test_glm4_stop_sequences() {
        let stop_seqs = get_stop_sequences_for_model("glm4");

        assert!(stop_seqs.contains(&"<|endoftext|>"));
        assert!(stop_seqs.contains(&"<|user|>"));
        assert!(stop_seqs.contains(&"<|observation|>"));
        assert!(stop_seqs.contains(&"<|system|>"));
    }

    #[test]
    fn test_qwen_stop_sequences() {
        let stop_seqs = get_stop_sequences_for_model("qwen");

        assert!(stop_seqs.contains(&"<|im_end|>"));
        assert!(stop_seqs.contains(&"<|endoftext|>"));
        assert!(stop_seqs.contains(&"<|im_start|>"));
    }

    #[test]
    fn test_glm4_multi_turn_conversation() {
        let messages = vec![
            Message::new(MessageRole::System, "You are a coding assistant.".to_string()),
            Message::new(
                MessageRole::User,
                "Write a hello world in Python.".to_string(),
            ),
            Message::new(
                MessageRole::Assistant,
                "print('Hello, World!')".to_string(),
            ),
            Message::new(MessageRole::User, "Now in Rust.".to_string()),
        ];

        let formatted = format_glm4(&messages);

        // Verify all parts are present in order
        let system_pos = formatted.find("<|system|>").unwrap();
        let user1_pos = formatted.find("<|user|>\nWrite a hello world").unwrap();
        let assistant_pos = formatted.find("<|assistant|>\nprint").unwrap();
        let user2_pos = formatted.find("<|user|>\nNow in Rust").unwrap();
        let final_assistant_pos = formatted.rfind("<|assistant|>\n").unwrap();

        assert!(system_pos < user1_pos);
        assert!(user1_pos < assistant_pos);
        assert!(assistant_pos < user2_pos);
        assert!(user2_pos < final_assistant_pos);
    }

    #[test]
    fn test_find_stop_sequence() {
        let stop_seqs = vec!["</s>".to_string(), "<|im_end|>".to_string()];

        assert_eq!(find_stop_sequence("hello world", &stop_seqs), None);
        assert_eq!(find_stop_sequence("hello</s>world", &stop_seqs), Some(5));
        assert_eq!(
            find_stop_sequence("hello<|im_end|>world", &stop_seqs),
            Some(5)
        );
    }

    #[test]
    fn test_truncate_at_stop_sequence() {
        let stop_seqs = vec!["</s>".to_string()];

        let mut text = "hello</s>world".to_string();
        truncate_at_stop_sequence(&mut text, &stop_seqs);
        assert_eq!(text, "hello");

        let mut text2 = "no stop here".to_string();
        truncate_at_stop_sequence(&mut text2, &stop_seqs);
        assert_eq!(text2, "no stop here");
    }
}
