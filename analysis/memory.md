# Workspace Memory
> Updated: 2026-01-27T02:55:20Z | Size: 21.9k chars

### Remember Tool Wiring
- `crates/g3-core/src/tools/memory.rs` [0..5000] - `execute_remember()`, `get_memory_path()`, `merge_memory()`
- `crates/g3-core/src/tool_definitions.rs` [11000..12000] - remember tool definition in `create_core_tools()`
- `crates/g3-core/src/tool_dispatch.rs` [48] - dispatch case for "remember"
- `crates/g3-core/src/prompts.rs` [4200..6500] - Workspace Memory section in native prompt
- `crates/g3-cli/src/lib.rs` [1472..1495] - `read_workspace_memory()` loads memory at startup

### Context Window & Compaction
- `crates/g3-core/src/context_window.rs` [0..815] - `ContextWindow`, `reset_with_summary()`, `should_compact()`, `thin_context()`
- `crates/g3-core/src/lib.rs` [0..132483] - `Agent` struct, `force_compact()`, `stream_completion_with_tools()`

### Session Storage & Continuation
- `crates/g3-core/src/session_continuation.rs` [0..541] - `SessionContinuation`, `save_continuation()`, `load_continuation()`
- `crates/g3-core/src/paths.rs` [0..133] - `get_session_logs_dir()`, `get_thinned_dir()`, `get_session_file()`
- `crates/g3-core/src/session.rs` - Session logging utilities

### Tool System
- `crates/g3-core/src/tool_definitions.rs` [0..544] - `create_core_tools()`, `create_tool_definitions()`, `ToolConfig`
- `crates/g3-core/src/tool_dispatch.rs` [0..73] - `dispatch_tool()` routing

### CLI Argument Parsing
- `crates/g3-cli/src/lib.rs` [270..380] - `Cli` struct with clap derive macros
- `crates/g3-cli/src/lib.rs` [1700..2200] - `run_interactive()` with `/` command handlers

### Streaming Markdown Formatter
- `crates/g3-cli/src/streaming_markdown.rs` [21500..22500] - `format_header()` processes headers with inline formatting
- `crates/g3-cli/tests/streaming_markdown_test.rs` - Tests for markdown formatting including `test_bold_inside_header`, `test_italic_inside_header`, `test_code_inside_header`, `test_mixed_formatting_inside_header`

### Auto-Memory Feature
- `crates/g3-core/src/lib.rs` [1459..1522] - `send_auto_memory_reminder()` sends reminder to LLM after tool calls
- `crates/g3-core/src/lib.rs` [1451..1454] - `set_auto_memory()` enables/disables auto-memory
- `crates/g3-core/src/lib.rs` [116] - `tool_calls_this_turn: Vec<String>` tracks tools called per turn
- `crates/g3-cli/src/lib.rs` [393] - `auto_memory: bool` CLI flag definition
- `crates/g3-cli/src/lib.rs` [641..642, 684..685] - Flag applied to agent in console/machine modes
- `crates/g3-cli/src/lib.rs` [1340..1350, 1394..1404] - Auto-memory reminder called in single-shot mode
- `crates/g3-cli/src/lib.rs` [1758, 1931, 2216] - Auto-memory reminder called in interactive mode

### Tool Call Tracking
- `crates/g3-core/src/lib.rs` [2843..2855] - `execute_tool_in_dir()` tracks all tool calls for auto-memory

### Agent Mode
- `crates/g3-cli/src/lib.rs` [695..910] - `run_agent_mode()` handles specialized agent execution with custom prompts
- `crates/g3-cli/src/lib.rs` [820..835] - Agent creation with `Agent::new_with_custom_prompt()`
- `crates/g3-cli/src/lib.rs` [837] - `agent.set_agent_mode()` enables agent-specific session tracking

### CLI Entry Points and Modes
- `crates/g3-cli/src/lib.rs` [0..140000] - `run()` main entry, `run_agent_mode()`, `run_accumulative_mode()`, `run_autonomous()`, `run_interactive()`, `run_interactive_machine()`
- `crates/g3-cli/src/lib.rs` - `execute_task()` (~line 1990), `execute_task_machine()` (~line 2262) - duplicated retry logic

### Retry Infrastructure
- `crates/g3-core/src/retry.rs` [0..12000] - `execute_with_retry()`, `retry_operation()`, `RetryConfig`, `RetryResult` - used by g3-planner but not g3-cli

### UI Abstraction Layer
- `crates/g3-core/src/ui_writer.rs` [0..4500] - `UiWriter` trait, `NullUiWriter`
- `crates/g3-cli/src/ui_writer_impl.rs` [0..14000] - `ConsoleUiWriter` implementation
- `crates/g3-cli/src/simple_output.rs` [0..1200] - `SimpleOutput` helper (separate from UiWriter)

### Feedback Extraction
- `crates/g3-core/src/feedback_extraction.rs` [0..22000] - `extract_coach_feedback()`, `try_extract_from_session_log()`, `try_extract_from_native_tool_call()`

### Streaming Utilities
- `crates/g3-core/src/streaming.rs` [0..10000] - `truncate_line()`, `truncate_for_display()`, `log_stream_error()`, `is_connection_error()`

### Background Process Management
- `crates/g3-core/src/background_process.rs` [0..3000] - `BackgroundProcessManager`, `start()`, `list()`, `is_running()`, `get()`, `remove()`
- Design: No `stop()` method - processes are stopped via shell tool using `kill <pid>`

### Unified Diff Application
- `crates/g3-core/src/utils.rs` [5000..15000] - `apply_unified_diff_to_string()`, `parse_unified_diff_hunks()`
- Handles multi-hunk diffs, CRLF normalization, range constraints

### Error Classification
- `crates/g3-core/src/error_handling.rs` [0..567 lines] - `classify_error()`, `ErrorType`, `RecoverableError`
- Priority order: rate limit > network > server > busy > timeout > token limit > context length
- Note: "Connection timeout" classifies as NetworkError (not Timeout) due to "connection" keyword priority

### CLI Module Extractions
- `crates/g3-cli/src/metrics.rs` [0..5416] - `TurnMetrics`, `format_elapsed_time()`, `generate_turn_histogram()`
- `crates/g3-cli/src/project_files.rs` [0..5577] - `read_agents_config()`, `read_project_readme()`, `read_workspace_memory()`, `extract_readme_heading()`
- `crates/g3-cli/src/coach_feedback.rs` [0..4025] - `extract_from_logs()` for coach-player loop feedback extraction

### Context Compaction
- `crates/g3-core/src/compaction.rs` [0..11213] - `perform_compaction()`, `CompactionResult`, `CompactionConfig`, `calculate_capped_summary_tokens()`, `should_disable_thinking()`, `build_summary_messages()`, `apply_summary_fallback_sequence()`
- Unified compaction used by both `force_compact()` and auto-compaction in `stream_completion_with_tools()`

### Streaming Markdown Formatter (Code Blocks)
- `crates/g3-cli/src/streaming_markdown.rs` [693..735] - `flush_incomplete()` handles unclosed blocks at end of stream
- `crates/g3-cli/src/streaming_markdown.rs` [654..675] - `emit_code_block()` joins block_buffer and highlights code
- `crates/g3-cli/src/streaming_markdown.rs` [439..462] - `process_in_code_block()` detects closing fence on newline
- Bug fix: closing ``` without trailing newline must be detected in flush_incomplete(), not just process_in_code_block()

### ACD (Aggressive Context Dehydration)
- `crates/g3-core/src/acd.rs` [0..22000] - `Fragment`, `Fragment::new()`, `Fragment::save()`, `Fragment::load()`, `generate_stub()`, `list_fragments()`, `get_latest_fragment_id()`
- `crates/g3-core/src/tools/acd.rs` [0..8500] - `execute_rehydrate()` tool implementation
- `crates/g3-core/src/paths.rs` [3200..3400] - `get_fragments_dir()` returns `.g3/sessions/<session_id>/fragments/`
- `crates/g3-core/src/compaction.rs` [195..240] - ACD integration in `perform_compaction()`, creates fragment and stub when `acd_enabled`
- `crates/g3-core/src/context_window.rs` [10100..10700] - `reset_with_summary_and_stub()` adds stub before summary
- `crates/g3-cli/src/lib.rs` [157..161] - `--acd` CLI flag
- `crates/g3-cli/src/lib.rs` [1476..1525] - `/fragments` and `/rehydrate` commands

### ACD Fragment Storage Format
```json
{
  "fragment_id": "abc123",
  "created_at": "2026-01-11T...",
  "messages": [...],
  "message_count": 47,
  "user_message_count": 23,
  "assistant_message_count": 24,
  "tool_call_summary": {"read_file": 4, "shell": 5},
  "estimated_tokens": 18500,
  "topics": ["implemented auth", "fixed bug"],
  "preceding_fragment_id": "xyz789"
}
```



### UTF-8 Safe String Slicing Pattern
**Problem**: Rust string slices (`&s[..n]`) use byte indices, not character indices. Multi-byte UTF-8 characters (emoji, bullets `•`, `×`, `⚡`) cause panics if sliced mid-character.

**Solution**: Use `char_indices()` to find byte boundaries:
```rust
// Get byte index of the Nth character
let byte_idx = s.char_indices()
    .nth(char_limit)
    .map(|(i, _)| i)
    .unwrap_or(s.len());
let truncated = &s[..byte_idx];

// For length checks, use chars().count() not len()
if s.chars().count() <= max_len { ... }
```



**Danger zones**: Display truncation, ACD stubs, user input handling, any string with non-ASCII characters.

### CLI Module Structure (Post-Refactor)
- `crates/g3-cli/src/lib.rs` [0..415] - Entry point, `run()`, mode dispatch, config loading
- `crates/g3-cli/src/cli_args.rs` [0..133] - `Cli` struct with clap derive macros, argument parsing
- `crates/g3-cli/src/autonomous.rs` [0..785] - `run_autonomous()`, coach-player feedback loop
- `crates/g3-cli/src/agent_mode.rs` [0..284] - `run_agent_mode()` specialized agent execution
- `crates/g3-cli/src/accumulative.rs` [0..343] - `run_accumulative_mode()` iterative requirements
- `crates/g3-cli/src/interactive.rs` [0..851] - `run_interactive()`, `run_interactive_machine()`, REPL with `/` commands
- `crates/g3-cli/src/task_execution.rs` [0..212] - `execute_task_with_retry()`, `OutputMode` enum - unified retry logic
- `crates/g3-cli/src/utils.rs` [0..91] - `display_welcome_message()`, `get_workspace_path()`

### Studio - Multi-Agent Workspace Manager
- `crates/studio/src/main.rs` [0..12500] - `cmd_run()`, `cmd_status()`, `cmd_accept()`, `cmd_discard()`, `extract_session_summary()`
- `crates/studio/src/session.rs` - `Session`, `SessionStatus`, session metadata management
- `crates/studio/src/git.rs` - `GitWorktree`, git worktree management for isolated agent sessions

**Session log format**: Session logs are stored at `<worktree>/.g3/sessions/<session_id>/session.json` with structure:
```json
{
  "context_window": {
    "conversation_history": [{"role": "...", "content": "..."}],
    "percentage_used": 45.2,
    "total_tokens": 200000,
    "used_tokens": 90400
  },
  "session_id": "...",
  "status": "...",
  "timestamp": "..."
}
```


### Workspace Memory Location
- Memory is now stored at `analysis/memory.md` (version controlled, shared across worktrees)
- `crates/g3-core/src/tools/memory.rs` - `get_memory_path()` returns `analysis/memory.md`
- `crates/g3-cli/src/project_files.rs` - `read_workspace_memory()` reads from `analysis/memory.md`

### Compact Tool Output
- `crates/g3-cli/src/ui_writer_impl.rs` - `print_tool_compact()` handles compact display for file ops and other tools
- `crates/g3-core/src/streaming.rs` - `format_*_summary()` functions for each tool type

### Racket Code Search Support
Tree-sitter based syntax-aware search for Racket `.rkt` files.

- `crates/g3-core/src/code_search/searcher.rs`
  - Racket parser init [~line 45] - `tree_sitter_racket::LANGUAGE`
  - Extension mapping [~line 90] - `.rkt`, `.rktl`, `.rktd` → "racket"

### Auto-Memory Reminder Format
Rich few-shot prompting for higher quality memory entries with per-symbol char ranges.

- `crates/g3-core/src/lib.rs`
  - `send_auto_memory_reminder()` [47800..48800] - MEMORY CHECKPOINT prompt with few-shot examples
- `crates/g3-core/src/prompts.rs`
  - Memory Format section [3800..4500] - system prompt template and examples

### Language-Specific Prompt Injection
Auto-detects programming languages in workspace and injects toolchain guidance.

- `crates/g3-cli/src/language_prompts.rs`
  - `LANGUAGE_PROMPTS` [12..19] - static array of (lang_name, extensions, prompt_content)
  - `detect_languages()` [22..32] - scans workspace for language files
  - `get_language_prompts_for_workspace()` [88..108] - returns formatted prompt for detected languages
  - `scan_directory_for_extensions()` [42..77] - recursive scan with depth limit (2), skips hidden/vendor dirs

- `prompts/langs/` - directory for language prompt markdown files
  - `racket.md` - Racket/raco toolchain guidance (compilation, testing, analysis, profiling)

- `crates/g3-cli/src/project_files.rs`
  - `combine_project_content()` [89..106] - now accepts `language_content` parameter

To add a new language:
1. Create `prompts/langs/<lang>.md` with toolchain guidance
2. Add entry to `LANGUAGE_PROMPTS` in `language_prompts.rs` with extensions

### Agent-Specific Language Prompts
Injects agent+language-specific guidance when running in agent mode in a workspace with detected languages.

- `crates/g3-cli/src/language_prompts.rs`
  - `AGENT_LANGUAGE_PROMPTS` [21..26] - static array of (agent_name, lang_name, prompt_content) tuples
  - `get_agent_language_prompt()` [115..121] - looks up prompt for specific agent+lang combo
  - `get_agent_language_prompts_for_workspace()` [124..137] - uses `detect_languages()` then looks up agent-specific prompts

- `crates/g3-cli/src/agent_mode.rs`
  - Lines 149-159 - calls `get_agent_language_prompts_for_workspace()` and appends to system prompt

- `prompts/langs/<agent>.<lang>.md` - file naming pattern for agent+lang prompts
  - `prompts/langs/carmack.racket.md` - Racket-specific guidance for carmack agent

To add a new agent+lang prompt:
1. Create `prompts/langs/<agent>.<lang>.md`
2. Add entry to `AGENT_LANGUAGE_PROMPTS` in `language_prompts.rs` with `include_str!`

### MockProvider for Testing
Configurable mock LLM provider for integration testing without real API calls.

- `crates/g3-providers/src/mock.rs`
  - `MockProvider` [220..320] - mock provider with response queue, request tracking
  - `MockResponse` [35..200] - configurable response with chunks and usage
  - `MockChunk` [45..100] - individual streaming chunk (content, finished, tool_calls)
  - `scenarios` module [410..480] - preset scenarios: `text_only_response()`, `multi_turn()`, `tool_then_response()`

- `crates/g3-core/tests/mock_provider_integration_test.rs`
  - `test_butler_bug_scenario()` - reproduces consecutive user messages bug
  - `test_text_only_response_saves_to_context()` - verifies text responses saved
  - `test_multi_turn_text_only_maintains_alternation()` - verifies user/assistant alternation

Usage pattern:
```rust
let provider = MockProvider::new()
    .with_response(MockResponse::text("Hello!"));
let mut registry = ProviderRegistry::new();
registry.register(provider);
let agent = Agent::new_for_test(config, NullUiWriter, registry).await?;
```

### G3 Status Message Formatting
Centralized formatting for all "g3:" prefixed system status messages.

- `crates/g3-cli/src/g3_status.rs`
  - `G3Status` - static methods for consistent status message formatting
  - `Status` enum - `Done`, `Failed`, `Error(String)`, `Custom(String)`, `Resolved`, `Insufficient`
  - `progress()` [64..76] - prints "g3: <message> ..." (no newline, stays on same line)
  - `progress_ln()` [79..90] - prints "g3: <message> ..." with newline
  - `done()` [93..101] - prints bold green "[done]"
  - `failed()` [104..111] - prints red "[failed]"
  - `error()` [114..122] - prints red "[error: <msg>]"
  - `status()` [125..152] - dispatches to appropriate status method
  - `complete()` [155..158] - one-shot progress + status
  - `info_inline()` [168..178] - ANSI escape to append to previous line
  - `format_status()` [181..214] - returns formatted status string
  - `resuming()` [227..236] - session resume message with cyan session ID
  - `resuming_summary()` [239..248] - resume with "(summary)" note

### ThinResult Struct
Semantic data for context thinning operations, replacing pre-formatted strings.

- `crates/g3-core/src/context_window.rs`
  - `ThinResult` [16..36] - struct with scope, before/after percentages, counts, chars_saved, had_changes
  - `thin_context_with_scope()` [373..450] - returns `ThinResult` instead of `(String, usize)`
  - `build_thin_result()` [720..740] - constructs `ThinResult` from operation data

- `crates/g3-core/src/ui_writer.rs`
  - `print_thin_result(&self, result: &ThinResult)` [31] - trait method for UI formatting

- `crates/g3-cli/src/g3_status.rs`
  - `Status::NoChanges` [42] - new status variant for thinning with no changes
  - `G3Status::thin_result()` [265..292] - formats ThinResult with proper colors/styling

### CLI Display Utilities
Shared display functions for interactive and agent modes.

- `crates/g3-cli/src/display.rs`
  - `format_workspace_path()` [9..17] - formats path with ~ for home dir
  - `print_workspace_path()` [20..29] - prints formatted workspace path
  - `LoadedContent` [32..39] - tracks loaded project files (README, AGENTS.md, Memory, include prompt)
  - `print_loaded_status()` [87..103] - prints "✓ README  ✓ AGENTS.md" status line
  - `print_project_heading()` [106..114] - prints project name from README

### Interactive Commands Module
Handles `/` commands in interactive mode (extracted from interactive.rs).

- `crates/g3-cli/src/commands.rs`
  - `handle_command()` [17..320] - dispatches `/help`, `/compact`, `/thinnify`, `/skinnify`, `/fragments`, `/rehydrate`, `/run`, `/dump`, `/clear`, `/readme`, `/stats`, `/resume`
  - Returns `Result<bool>` - true if command handled and loop should continue

### Streaming State Management
State structs for the main streaming loop in `stream_completion_with_tools()`.

- `crates/g3-core/src/streaming.rs`
  - `StreamingState` [17..42] - cross-iteration state: `full_response`, `first_token_time`, `stream_start`, `iteration_count`, `response_started`, `any_tool_executed`, `assistant_message_added`, `turn_accumulated_usage`
  - `IterationState` [65..90] - per-iteration state: `parser`, `current_response`, `tool_executed`, `chunks_received`, `raw_chunks`, `accumulated_usage`, `stream_stop_reason`
  - `MAX_ITERATIONS` [15] - constant (400) for loop safety

- `crates/g3-core/src/lib.rs`
  - `stream_completion_with_tools()` [1879..2712] - 834-line main streaming loop, uses `state: StreamingState` and `iter: IterationState`

### Tool Output Formatting
Centralized logic for determining how to display tool execution results.

- `crates/g3-core/src/streaming.rs`
  - `ToolOutputFormat` [100..112] - enum: SelfHandled, Compact(String), Regular
  - `format_tool_result_summary()` [114..145] - returns ToolOutputFormat based on tool name and success
  - `is_compact_tool()` [147..162] - checks if tool uses one-line summaries (read_file, write_file, str_replace, etc.)
  - `is_self_handled_tool()` [164..167] - checks if tool handles own output (todo_read, todo_write)
  - `format_compact_tool_summary()` [169..185] - dispatches to format_*_summary() based on tool name
  - `parse_diff_stats()` [187..210] - parses "+N insertions | -M deletions" from str_replace result

### Prompt Cache Statistics Tracking
Tracks prompt/prefix caching efficacy across Anthropic and OpenAI providers.

- `crates/g3-providers/src/lib.rs`
  - `Usage` [195..210] - added `cache_creation_tokens` and `cache_read_tokens` fields with `#[serde(default)]`

- `crates/g3-providers/src/anthropic.rs`
  - `AnthropicUsage` [944..956] - parses `cache_creation_input_tokens` and `cache_read_input_tokens`

- `crates/g3-providers/src/openai.rs`
  - `OpenAIUsage` [494..510] - parses `prompt_tokens_details.cached_tokens`
  - `OpenAIPromptTokensDetails` [504..510] - nested struct for prompt token details

- `crates/g3-core/src/lib.rs`
  - `CacheStats` [75..90] - cumulative cache statistics struct with `total_cache_creation_tokens`, `total_cache_read_tokens`, `total_input_tokens`, `cache_hit_calls`, `total_calls`
  - `Agent.cache_stats` [106] - field tracking cumulative cache stats
  - Cache stats updated in `stream_completion_with_tools()` [2140..2150] when usage data received

- `crates/g3-core/src/stats.rs`
  - `AgentStatsSnapshot.cache_stats` [20] - reference to cache stats for formatting
  - `format_cache_stats()` [189..230] - formats cache statistics section with hit rate and efficiency metrics

### Embedded Provider (Local LLM via llama.cpp)
Local model inference using llama-cpp-rs bindings with Metal acceleration on macOS.

- `crates/g3-providers/src/embedded.rs`
  - `EmbeddedProvider` [22..85] - struct with session, model_name, max_tokens, temperature, context_length
  - `new()` [26..85] - constructor, handles tilde expansion, auto-downloads Qwen if missing
  - `format_messages()` [87..175] - converts Message[] to prompt string, supports Qwen/Mistral/Llama templates
  - `get_stop_sequences()` [280..340] - returns model-specific stop tokens
  - `generate_completion()` [177..278] - non-streaming inference with timeout
  - `stream()` [560..780] - streaming inference via spawn_blocking + mpsc channel

**Known Issues (as of 2025-01):**
- Provider name hardcoded as `"embedded"` instead of `"embedded.{name}"` format
- Missing GLM-4 chat template (uses `[gMASK]<sop><|role|>` NOT ChatML)
- Missing `has_native_tool_calling()` override (defaults to false)
- No streaming usage tracking (unlike Anthropic)
- No tool streaming hints (`make_tool_streaming_hint()` not used)

### Chat Template Formats (Embedded Provider)
| Model | Format | Start Token | End Token |
|-------|--------|-------------|----------|
| Qwen | ChatML | `<\|im_start\|>role\n` | `<\|im_end\|>` |
| GLM-4 | ChatGLM4 | `[gMASK]<sop><\|role\|>\n` | `<\|endoftext\|>` |
| Mistral | Instruct | `<s>[INST]` | `[/INST]` |
| Llama | Llama2 | `<<SYS>>` | `<</SYS>>` |

### GLM-4 Model Downloads
Recommended GGUF models for Mac M4 Max with 128GB unified memory.

**Download commands:**
```bash
# GLM-4 9B Q8_0 (~10GB) - Very capable, fast
python3 -m huggingface_hub.commands.huggingface_cli download bartowski/THUDM_GLM-4-9B-0414-GGUF \
  --include "THUDM_GLM-4-9B-0414-Q8_0.gguf" --local-dir ~/.g3/models/

# GLM-4 32B Q6_K_L (~27GB) - TOP TIER for coding/reasoning
python3 -m huggingface_hub.commands.huggingface_cli download bartowski/THUDM_GLM-4-32B-0414-GGUF \
  --include "THUDM_GLM-4-32B-0414-Q6_K_L.gguf" --local-dir ~/.g3/models/

# Qwen3 4B Q4_K_M (~2.3GB) - Small but rivals 72B performance
python3 -m huggingface_hub.commands.huggingface_cli download Qwen/Qwen3-4B-GGUF \
  --include "qwen3-4b-q4_k_m.gguf" --local-dir ~/.g3/models/
```

**Config example:**
```toml
[providers.embedded.glm4]
model_path = "~/.g3/models/THUDM_GLM-4-32B-0414-Q6_K_L.gguf"
model_type = "glm4"
context_length = 32768
max_tokens = 4096
gpu_layers = 99
```