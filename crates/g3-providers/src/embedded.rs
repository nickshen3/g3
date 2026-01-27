use crate::{
    CompletionRequest, CompletionResponse, CompletionStream, LLMProvider, Message,
    MessageRole, Usage,
    streaming::{make_text_chunk, make_final_chunk_with_reason},
};
use anyhow::Result;
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error};

/// Global llama.cpp backend - can only be initialized once per process
static LLAMA_BACKEND: OnceLock<Arc<LlamaBackend>> = OnceLock::new();

/// Get or initialize the global llama.cpp backend
fn get_or_init_backend() -> Result<Arc<LlamaBackend>> {
    // Check if already initialized
    if let Some(backend) = LLAMA_BACKEND.get() {
        return Ok(Arc::clone(backend));
    }
    
    // Suppress llama.cpp's verbose logging to stderr before initialization
    unsafe {
        unsafe extern "C" fn void_log(
            _level: std::ffi::c_int,
            _text: *const std::os::raw::c_char,
            _user_data: *mut std::os::raw::c_void,
        ) {
            // Intentionally empty - suppress all llama.cpp logging
        }
        // Call the underlying C function directly
        extern "C" { fn llama_log_set(log_callback: Option<unsafe extern "C" fn(std::ffi::c_int, *const std::os::raw::c_char, *mut std::os::raw::c_void)>, user_data: *mut std::os::raw::c_void); }
        llama_log_set(Some(void_log), std::ptr::null_mut());
    }
    
    // Try to initialize
    debug!("Initializing llama.cpp backend...");
    let backend = LlamaBackend::init()
        .map_err(|e| anyhow::anyhow!("Failed to initialize llama.cpp backend: {:?}", e))?;
    
    // Store it (ignore if another thread beat us to it)
    let _ = LLAMA_BACKEND.set(Arc::new(backend));
    let backend = LLAMA_BACKEND.get().expect("backend was just set");
    Ok(Arc::clone(backend))
}

/// Embedded LLM provider using llama.cpp with Metal acceleration on macOS.
/// 
/// Supports multiple model families with their native chat templates:
/// - Qwen (ChatML format)
/// - GLM-4 (ChatGLM4 format)
/// - Mistral (Instruct format)
/// - Llama/CodeLlama (Llama2 format)
pub struct EmbeddedProvider {
    /// Provider name in format "embedded.{config_name}"
    name: String,
    /// The loaded model
    model: Arc<LlamaModel>,
    /// The llama.cpp backend (must be kept alive)
    backend: Arc<LlamaBackend>,
    /// Model type identifier (e.g., "qwen", "glm4", "mistral")
    model_type: String,
    /// Full model name for display
    model_name: String,
    /// Maximum tokens to generate (None = auto-calculate)
    max_tokens: Option<u32>,
    /// Sampling temperature
    temperature: f32,
    /// Context window size
    context_length: u32,
    /// Number of threads
    threads: Option<u32>,
}

impl EmbeddedProvider {
    /// Create a new embedded provider with default naming.
    /// 
    /// The provider will be registered as "embedded" (legacy behavior).
    /// For proper multi-provider support, use `new_with_name()` instead.
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
    /// 
    /// # Arguments
    /// * `name` - Provider name (e.g., "embedded.glm4", "embedded.qwen")
    /// * `model_path` - Path to the GGUF model file (supports ~ expansion)
    /// * `model_type` - Model family identifier ("qwen", "glm4", "glm", "mistral", "llama", etc.)
    /// * `context_length` - Context window size (default: auto-detected from GGUF)
    /// * `max_tokens` - Maximum tokens to generate (default: min(4096, context/4))
    /// * `temperature` - Sampling temperature (default: 0.1)
    /// * `gpu_layers` - Number of layers to offload to GPU (default: 99 for Apple Silicon)
    /// * `threads` - Number of CPU threads for inference
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

        // Expand tilde in path
        let expanded_path = shellexpand::tilde(&model_path);
        let model_path_buf = PathBuf::from(expanded_path.as_ref());

        if !model_path_buf.exists() {
            anyhow::bail!("Model file not found: {}", model_path_buf.display());
        }

        // Get or initialize the global llama.cpp backend
        let backend = get_or_init_backend()?;

        // Set up model parameters
        let n_gpu_layers = gpu_layers.unwrap_or(99);
        let model_params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
        debug!("Using {} GPU layers", n_gpu_layers);

        // Load the model
        debug!("Loading model...");
        let model = LlamaModel::load_from_file(&backend, &model_path_buf, &model_params)
            .map_err(|e| anyhow::anyhow!("Failed to load model: {:?}", e))?;

        // Auto-detect context length from GGUF metadata, or use provided value
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

    /// Format messages according to the model's native chat template.
    fn format_messages(&self, messages: &[Message]) -> String {
        let model_type = &self.model_type;

        if model_type.contains("glm") {
            self.format_glm4_messages(messages)
        } else if model_type.contains("qwen") {
            self.format_qwen_messages(messages)
        } else if model_type.contains("mistral") {
            self.format_mistral_messages(messages)
        } else {
            // Default to Llama format
            self.format_llama_messages(messages)
        }
    }

    /// GLM-4 ChatGLM4 format: [gMASK]<sop><|role|>\ncontent
    fn format_glm4_messages(&self, messages: &[Message]) -> String {
        let mut formatted = String::from("[gMASK]<sop>");

        for message in messages {
            let role = match message.role {
                MessageRole::System => "<|system|>",
                MessageRole::User => "<|user|>",
                MessageRole::Assistant => "<|assistant|>",
            };
            formatted.push_str(&format!("{}\n{}", role, message.content));
        }

        // Add the start of assistant response
        formatted.push_str("<|assistant|>\n");
        formatted
    }

    /// Qwen ChatML format: <|im_start|>role\ncontent<|im_end|>
    fn format_qwen_messages(&self, messages: &[Message]) -> String {
        let mut formatted = String::new();

        for message in messages {
            let role = match message.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
            };

            formatted.push_str(&format!(
                "<|im_start|>{}\n{}<|im_end|>\n",
                role, message.content
            ));
        }

        // Add the start of assistant response
        formatted.push_str("<|im_start|>assistant\n");
        formatted
    }

    /// Mistral Instruct format: <s>[INST] ... [/INST] response</s>
    fn format_mistral_messages(&self, messages: &[Message]) -> String {
        let mut formatted = String::new();
        let mut in_conversation = false;

        for (i, message) in messages.iter().enumerate() {
            match message.role {
                MessageRole::System => {
                    // Mistral doesn't have a special system token, include it at the start
                    if i == 0 {
                        formatted.push_str("<s>[INST] ");
                        formatted.push_str(&message.content);
                        formatted.push_str("\n\n");
                        in_conversation = true;
                    }
                }
                MessageRole::User => {
                    if !in_conversation {
                        formatted.push_str("<s>[INST] ");
                    }
                    formatted.push_str(&message.content);
                    formatted.push_str(" [/INST]");
                    in_conversation = false;
                }
                MessageRole::Assistant => {
                    formatted.push(' ');
                    formatted.push_str(&message.content);
                    formatted.push_str("</s> ");
                    in_conversation = false;
                }
            }
        }

        // If the last message was from user, add a space for the assistant's response
        if messages
            .last()
            .is_some_and(|m| matches!(m.role, MessageRole::User))
        {
            formatted.push(' ');
        }

        formatted
    }

    /// Llama/CodeLlama format: [INST] <<SYS>>\nsystem<</SYS>>\n\nuser [/INST]
    fn format_llama_messages(&self, messages: &[Message]) -> String {
        let mut formatted = String::new();

        for message in messages {
            match message.role {
                MessageRole::System => {
                    formatted.push_str(&format!(
                        "[INST] <<SYS>>\n{}\n<</SYS>>\n\n",
                        message.content
                    ));
                }
                MessageRole::User => {
                    formatted.push_str(&format!("{} [/INST] ", message.content));
                }
                MessageRole::Assistant => {
                    formatted.push_str(&format!("{} </s><s>[INST] ", message.content));
                }
            }
        }
        formatted
    }

    /// Estimate token count from text (rough approximation: ~4 chars per token)
    fn estimate_tokens(&self, text: &str) -> u32 {
        (text.len() as f32 / 4.0).ceil() as u32
    }

    /// Get stop sequences based on model type.
    fn get_stop_sequences(&self) -> Vec<&'static str> {
        let model_type = &self.model_type;

        if model_type.contains("glm") {
            vec![
                "<|endoftext|>",  // GLM end of text
                "<|user|>",       // Start of new user turn
                "<|observation|>", // Tool observation (shouldn't appear in response)
                "<|system|>",     // System message (shouldn't appear in response)
            ]
        } else if model_type.contains("qwen") {
            vec![
                "<|im_end|>",     // Qwen ChatML format end token
                "<|endoftext|>",  // Alternative end token
                "</s>",           // Generic end of sequence
                "<|im_start|>",   // Start of new message (shouldn't appear in response)
            ]
        } else if model_type.contains("codellama") || model_type.contains("code-llama") {
            vec![
                "</s>",      // End of sequence
                "[/INST]",   // End of instruction
                "<</SYS>>",  // End of system message
                "[INST]",    // Start of new instruction
                "<<SYS>>",   // Start of system
            ]
        } else if model_type.contains("llama") {
            vec![
                "</s>",            // End of sequence
                "[/INST]",         // End of instruction
                "<</SYS>>",        // End of system message
                "### Human:",      // Conversation format
                "### Assistant:",  // Conversation format
                "[INST]",          // Start of new instruction
            ]
        } else if model_type.contains("mistral") {
            vec![
                "</s>",        // End of sequence
                "[/INST]",     // End of instruction
                "<|im_end|>",  // ChatML format (some Mistral fine-tunes)
            ]
        } else if model_type.contains("vicuna") || model_type.contains("wizard") {
            vec![
                "### Human:",      // Conversation format
                "### Assistant:",  // Conversation format
                "USER:",           // Alternative format
                "ASSISTANT:",      // Alternative format
                "</s>",            // End of sequence
            ]
        } else if model_type.contains("alpaca") {
            vec![
                "### Instruction:",  // Alpaca format
                "### Response:",     // Alpaca format
                "### Input:",        // Alpaca format
                "</s>",              // End of sequence
            ]
        } else {
            // Generic/unknown model - use common stop sequences
            vec![
                "</s>",            // Most common end sequence
                "<|endoftext|>",   // GPT-style
                "<|im_end|>",      // ChatML
                "### Human:",      // Common conversation format
                "### Assistant:",  // Common conversation format
                "[/INST]",         // Instruction format
                "<</SYS>>",        // System format
            ]
        }
    }

    /// Get the effective max tokens for generation
    fn effective_max_tokens(&self) -> u32 {
        self.max_tokens
            .unwrap_or_else(|| std::cmp::min(4096, self.context_length / 4))
    }
}

#[async_trait::async_trait]
impl LLMProvider for EmbeddedProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        debug!(
            "Processing completion request with {} messages",
            request.messages.len()
        );

        let prompt = self.format_messages(&request.messages);
        let max_tokens = request.max_tokens.unwrap_or_else(|| self.effective_max_tokens());
        let temperature = request.temperature.unwrap_or(self.temperature);

        debug!("Formatted prompt length: {} chars", prompt.len());
        
        // Estimate prompt tokens before moving prompt into closure
        let prompt_tokens = self.estimate_tokens(&prompt);

        // Clone what we need for the blocking task
        let model = self.model.clone();
        let backend = self.backend.clone();
        let context_length = self.context_length;
        let threads = self.threads;
        let stop_sequences: Vec<String> = self.get_stop_sequences().iter().map(|s| s.to_string()).collect();

        let (content, completion_tokens) = tokio::task::spawn_blocking(move || {
            // Create context for this completion
            let n_ctx = NonZeroU32::new(context_length).unwrap_or(NonZeroU32::new(4096).unwrap());
            let mut ctx_params = LlamaContextParams::default()
                .with_n_ctx(Some(n_ctx))
                .with_n_batch(context_length);  // Batch size must accommodate full prompt
            if let Some(n_threads) = threads {
                ctx_params = ctx_params.with_n_threads(n_threads as i32);
            }

            let mut ctx = model
                .new_context(&backend, ctx_params)
                .map_err(|e| anyhow::anyhow!("Failed to create context: {:?}", e))?;

            // Tokenize the prompt
            let tokens = model
                .str_to_token(&prompt, AddBos::Always)
                .map_err(|e| anyhow::anyhow!("Failed to tokenize: {:?}", e))?;

            debug!("Tokenized prompt: {} tokens", tokens.len());

            // Create batch large enough for the prompt tokens
            // The batch size must be at least as large as the number of tokens we're adding
            let batch_size = std::cmp::max(512, tokens.len());
            let mut batch = LlamaBatch::new(batch_size, 1);
            for (i, token) in tokens.iter().enumerate() {
                batch
                    .add(*token, i as i32, &[0], i == tokens.len() - 1)
                    .map_err(|e| anyhow::anyhow!("Failed to add token to batch: {:?}", e))?;
            }

            // Decode the prompt
            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("Failed to decode prompt: {:?}", e))?;

            // Set up sampler
            let mut sampler = LlamaSampler::chain_simple([
                LlamaSampler::temp(temperature),
                LlamaSampler::dist(1234),
            ]);

            // Generate tokens
            let mut generated_text = String::new();
            let mut n_cur = tokens.len() as i32;
            let mut token_count = 0u32;

            for _ in 0..max_tokens {
                let new_token = sampler.sample(&ctx, batch.n_tokens() - 1);
                sampler.accept(new_token);

                // Check for end of generation
                if model.is_eog_token(new_token) {
                    debug!("Hit end-of-generation token at {} tokens", token_count);
                    break;
                }

                // Decode token to string
                let token_str = model.token_to_str(new_token, Special::Tokenize)
                    .unwrap_or_default();
                generated_text.push_str(&token_str);
                token_count += 1;

                // Check for stop sequences
                let mut hit_stop = false;
                for stop_seq in &stop_sequences {
                    if generated_text.contains(stop_seq) {
                        debug!("Hit stop sequence '{}' at {} tokens", stop_seq, token_count);
                        hit_stop = true;
                        break;
                    }
                }
                if hit_stop {
                    break;
                }

                // Prepare next batch
                batch.clear();
                batch
                    .add(new_token, n_cur, &[0], true)
                    .map_err(|e| anyhow::anyhow!("Failed to add token to batch: {:?}", e))?;
                n_cur += 1;

                ctx.decode(&mut batch)
                    .map_err(|e| anyhow::anyhow!("Failed to decode: {:?}", e))?;
            }

            // Clean stop sequences from output
            for stop_seq in &stop_sequences {
                if let Some(pos) = generated_text.find(stop_seq) {
                    generated_text.truncate(pos);
                    break;
                }
            }

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
            model: self.model_name.clone(),
        })
    }

    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream> {
        debug!(
            "Processing streaming request with {} messages",
            request.messages.len()
        );

        let prompt = self.format_messages(&request.messages);
        let max_tokens = request.max_tokens.unwrap_or_else(|| self.effective_max_tokens());
        let temperature = request.temperature.unwrap_or(self.temperature);

        // Estimate prompt tokens for usage tracking
        let prompt_tokens = self.estimate_tokens(&prompt);

        let (tx, rx) = mpsc::channel(100);

        // Clone what we need for the blocking task
        let model = self.model.clone();
        let backend = self.backend.clone();
        let context_length = self.context_length;
        let threads = self.threads;
        let stop_sequences: Vec<String> = self.get_stop_sequences().iter().map(|s| s.to_string()).collect();

        tokio::task::spawn_blocking(move || {
            // Create context for this completion
            let n_ctx = NonZeroU32::new(context_length).unwrap_or(NonZeroU32::new(4096).unwrap());
            let mut ctx_params = LlamaContextParams::default()
                .with_n_ctx(Some(n_ctx))
                .with_n_batch(context_length);  // Batch size must accommodate full prompt
            if let Some(n_threads) = threads {
                ctx_params = ctx_params.with_n_threads(n_threads as i32);
            }

            let mut ctx = match model.new_context(&backend, ctx_params) {
                Ok(ctx) => ctx,
                Err(e) => {
                    let _ = tx.blocking_send(Err(anyhow::anyhow!("Failed to create context: {:?}", e)));
                    return;
                }
            };

            // Tokenize the prompt
            let tokens = match model.str_to_token(&prompt, AddBos::Always) {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.blocking_send(Err(anyhow::anyhow!("Failed to tokenize: {:?}", e)));
                    return;
                }
            };

            debug!("Tokenized prompt: {} tokens", tokens.len());

            // Create batch large enough for the prompt tokens
            // The batch size must be at least as large as the number of tokens we're adding
            let batch_size = std::cmp::max(512, tokens.len());
            let mut batch = LlamaBatch::new(batch_size, 1);
            for (i, token) in tokens.iter().enumerate() {
                if let Err(e) = batch.add(*token, i as i32, &[0], i == tokens.len() - 1) {
                    let _ = tx.blocking_send(Err(anyhow::anyhow!("Failed to add token to batch: {:?}", e)));
                    return;
                }
            }

            // Decode the prompt
            if let Err(e) = ctx.decode(&mut batch) {
                let _ = tx.blocking_send(Err(anyhow::anyhow!("Failed to decode prompt: {:?}", e)));
                return;
            }

            // Set up sampler
            let mut sampler = LlamaSampler::chain_simple([
                LlamaSampler::temp(temperature),
                LlamaSampler::dist(1234),
            ]);

            // Generate tokens
            let mut accumulated_text = String::new();
            let mut n_cur = tokens.len() as i32;
            let mut token_count = 0u32;
            let mut stop_reason: Option<String> = None;

            for _ in 0..max_tokens {
                let new_token = sampler.sample(&ctx, batch.n_tokens() - 1);
                sampler.accept(new_token);

                // Check for end of generation
                if model.is_eog_token(new_token) {
                    debug!("Hit end-of-generation token at {} tokens", token_count);
                    stop_reason = Some("end_turn".to_string());
                    break;
                }

                // Decode token to string
                let token_str = model.token_to_str(new_token, Special::Tokenize)
                    .unwrap_or_default();
                
                accumulated_text.push_str(&token_str);
                token_count += 1;

                // Check for stop sequences
                let mut hit_stop = false;
                for stop_seq in &stop_sequences {
                    if accumulated_text.contains(stop_seq) {
                        debug!("Hit stop sequence '{}' at {} tokens", stop_seq, token_count);
                        hit_stop = true;
                        stop_reason = Some("stop_sequence".to_string());
                        break;
                    }
                }

                if hit_stop {
                    // Send any remaining clean content
                    let mut clean_text = accumulated_text.clone();
                    for stop_seq in &stop_sequences {
                        if let Some(pos) = clean_text.find(stop_seq) {
                            clean_text.truncate(pos);
                            break;
                        }
                    }
                    // We've been sending incrementally, so just break
                    break;
                }

                // Send the token
                let chunk = make_text_chunk(token_str);
                if tx.blocking_send(Ok(chunk)).is_err() {
                    break;
                }

                // Check token limit
                if token_count >= max_tokens {
                    debug!("Reached max token limit: {}", max_tokens);
                    stop_reason = Some("max_tokens".to_string());
                    break;
                }

                // Prepare next batch
                batch.clear();
                if let Err(e) = batch.add(new_token, n_cur, &[0], true) {
                    error!("Failed to add token to batch: {:?}", e);
                    break;
                }
                n_cur += 1;

                if let Err(e) = ctx.decode(&mut batch) {
                    error!("Failed to decode: {:?}", e);
                    break;
                }
            }

            // If no stop reason set, it was end_turn (natural completion)
            if stop_reason.is_none() {
                stop_reason = Some("end_turn".to_string());
            }

            // Send final chunk with usage information
            let usage = Usage {
                prompt_tokens,
                completion_tokens: token_count,
                total_tokens: prompt_tokens + token_count,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            };
            let final_chunk = make_final_chunk_with_reason(vec![], Some(usage), stop_reason);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_glm4_messages() {
        let messages = vec![
            Message::new(MessageRole::System, "You are a helpful assistant.".to_string()),
            Message::new(MessageRole::User, "Hello!".to_string()),
        ];

        let formatted = format_glm4_messages_standalone(&messages);
        
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

        let formatted = format_qwen_messages_standalone(&messages);
        
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

        let formatted = format_mistral_messages_standalone(&messages);
        
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

        let formatted = format_llama_messages_standalone(&messages);
        
        assert!(formatted.contains("<<SYS>>"));
        assert!(formatted.contains("You are a helpful assistant."));
        assert!(formatted.contains("<</SYS>>"));
        assert!(formatted.contains("Hello!"));
        assert!(formatted.contains("[/INST]"));
    }

    #[test]
    fn test_glm4_stop_sequences() {
        let stop_seqs = get_stop_sequences_for_model_type("glm4");
        
        assert!(stop_seqs.contains(&"<|endoftext|>"));
        assert!(stop_seqs.contains(&"<|user|>"));
        assert!(stop_seqs.contains(&"<|observation|>"));
        assert!(stop_seqs.contains(&"<|system|>"));
    }

    #[test]
    fn test_qwen_stop_sequences() {
        let stop_seqs = get_stop_sequences_for_model_type("qwen");
        
        assert!(stop_seqs.contains(&"<|im_end|>"));
        assert!(stop_seqs.contains(&"<|endoftext|>"));
        assert!(stop_seqs.contains(&"<|im_start|>"));
    }

    #[test]
    fn test_glm4_multi_turn_conversation() {
        let messages = vec![
            Message::new(MessageRole::System, "You are a coding assistant.".to_string()),
            Message::new(MessageRole::User, "Write a hello world in Python.".to_string()),
            Message::new(MessageRole::Assistant, "print('Hello, World!')".to_string()),
            Message::new(MessageRole::User, "Now in Rust.".to_string()),
        ];

        let formatted = format_glm4_messages_standalone(&messages);
        
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

    // Standalone formatting functions for testing without needing a full provider
    fn format_glm4_messages_standalone(messages: &[Message]) -> String {
        let mut formatted = String::from("[gMASK]<sop>");
        for message in messages {
            let role = match message.role {
                MessageRole::System => "<|system|>",
                MessageRole::User => "<|user|>",
                MessageRole::Assistant => "<|assistant|>",
            };
            formatted.push_str(&format!("{}\n{}", role, message.content));
        }
        formatted.push_str("<|assistant|>\n");
        formatted
    }

    fn format_qwen_messages_standalone(messages: &[Message]) -> String {
        let mut formatted = String::new();
        for message in messages {
            let role = match message.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
            };
            formatted.push_str(&format!(
                "<|im_start|>{}\n{}<|im_end|>\n",
                role, message.content
            ));
        }
        formatted.push_str("<|im_start|>assistant\n");
        formatted
    }

    fn format_mistral_messages_standalone(messages: &[Message]) -> String {
        let mut formatted = String::new();
        let mut in_conversation = false;
        for (i, message) in messages.iter().enumerate() {
            match message.role {
                MessageRole::System => {
                    if i == 0 {
                        formatted.push_str("<s>[INST] ");
                        formatted.push_str(&message.content);
                        formatted.push_str("\n\n");
                        in_conversation = true;
                    }
                }
                MessageRole::User => {
                    if !in_conversation {
                        formatted.push_str("<s>[INST] ");
                    }
                    formatted.push_str(&message.content);
                    formatted.push_str(" [/INST]");
                    in_conversation = false;
                }
                MessageRole::Assistant => {
                    formatted.push(' ');
                    formatted.push_str(&message.content);
                    formatted.push_str("</s> ");
                    in_conversation = false;
                }
            }
        }
        if messages.last().is_some_and(|m| matches!(m.role, MessageRole::User)) {
            formatted.push(' ');
        }
        formatted
    }

    fn format_llama_messages_standalone(messages: &[Message]) -> String {
        let mut formatted = String::new();
        for message in messages {
            match message.role {
                MessageRole::System => {
                    formatted.push_str(&format!(
                        "[INST] <<SYS>>\n{}\n<</SYS>>\n\n",
                        message.content
                    ));
                }
                MessageRole::User => {
                    formatted.push_str(&format!("{} [/INST] ", message.content));
                }
                MessageRole::Assistant => {
                    formatted.push_str(&format!("{} </s><s>[INST] ", message.content));
                }
            }
        }
        formatted
    }

    fn get_stop_sequences_for_model_type(model_type: &str) -> Vec<&'static str> {
        if model_type.contains("glm") {
            vec![
                "<|endoftext|>",
                "<|user|>",
                "<|observation|>",
                "<|system|>",
            ]
        } else if model_type.contains("qwen") {
            vec![
                "<|im_end|>",
                "<|endoftext|>",
                "</s>",
                "<|im_start|>",
            ]
        } else if model_type.contains("mistral") {
            vec![
                "</s>",
                "[/INST]",
                "<|im_end|>",
            ]
        } else {
            vec![
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
}
