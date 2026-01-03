# AGENTS.md - Machine Instructions for G3

**Last updated**: January 2025  
**Purpose**: Enable AI agents to work safely and effectively with this codebase

## System Overview

G3 is an AI coding agent built in Rust. It uses LLM providers to execute tasks through a tool-based interface. The codebase is organized as a Cargo workspace with 9 crates.

### Quick Reference

| Crate | Purpose | Stability |
|-------|---------|----------|
| `g3-core` | Agent engine, tools, context management | Stable |
| `g3-providers` | LLM provider abstractions | Stable |
| `g3-cli` | Command-line interface | Stable |
| `g3-config` | Configuration management | Stable |
| `g3-execution` | Code execution | Stable |
| `g3-computer-control` | Computer automation | Experimental |
| `g3-planner` | Planning mode | Stable |
| `g3-ensembles` | Multi-agent (flock) mode | Experimental |
| `g3-console` | Web monitoring console | Experimental |

## Critical Invariants

### MUST Hold

1. **Tool calls must be valid JSON** - The streaming parser expects well-formed tool calls
2. **Context window limits must be respected** - Exceeding limits causes API errors
3. **Provider trait implementations must be Send + Sync** - Required for async runtime
4. **Session IDs must be unique** - Used for log file paths and TODO scoping
5. **File paths in tools support tilde expansion** - `~` expands to home directory

### MUST NOT Do

1. **Never block the async runtime** - Use `tokio::spawn` for CPU-intensive work
2. **Never store secrets in logs** - API keys are redacted in error logs
3. **Never modify files outside working directory without explicit permission**
4. **Never assume tool results fit in context** - Large results are thinned automatically

## Recommended Entry Points

### For Understanding the System

1. `src/main.rs` - Entry point (trivial)
2. `crates/g3-cli/src/lib.rs` - CLI logic and execution modes
3. `crates/g3-core/src/lib.rs` - Agent struct and orchestration
4. `crates/g3-providers/src/lib.rs` - Provider trait definition

### For Adding Features

1. **New tool**: `crates/g3-core/src/tool_definitions.rs` → `crates/g3-core/src/tools/`
2. **New provider**: `crates/g3-providers/src/` → implement `LLMProvider` trait
3. **New CLI mode**: `crates/g3-cli/src/lib.rs`
4. **New config option**: `crates/g3-config/src/lib.rs`

### For Debugging

1. Session logs: `.g3/sessions/<session_id>/session.json`
2. Error logs: `logs/errors/`
3. Context state: Use `/stats` command in interactive mode

## Dangerous/Subtle Code Paths

### Context Window Management (`g3-core/src/context_window.rs`)

- **Thinning**: Automatically replaces large tool results with file references
- **Summarization**: Compresses conversation history at 80% capacity
- **Token estimation**: Uses character-based heuristics, not exact tokenization
- **Risk**: Incorrect token estimates can cause context overflow

### Streaming Parser (`g3-core/src/streaming_parser.rs`)

- Parses LLM responses in real-time for tool calls
- Must handle partial JSON across chunk boundaries
- **Risk**: Malformed responses can cause parsing failures

### Tool Dispatch (`g3-core/src/tool_dispatch.rs`)

- Routes tool calls to implementations
- Handles both native and JSON-based tool calling
- **Risk**: Missing dispatch cases cause silent failures

### Retry Logic (`g3-core/src/retry.rs`)

- Exponential backoff with jitter
- Different configs for interactive vs autonomous mode
- **Risk**: Aggressive retries can hit rate limits harder

## Performance Constraints

1. **Streaming is preferred** - Non-streaming requests block UI
2. **Tool results are size-limited** - Large outputs are truncated or thinned
3. **Concurrent tool calls** - Enabled by `allow_multiple_tool_calls` config
4. **Background processes** - Long-running commands use `background_process` tool

## Testing Strategy

### Test Locations

- Unit tests: `crates/*/tests/`
- Integration tests: `crates/*/tests/`
- Test fixtures: `examples/test_code/`

### Running Tests

```bash
# All tests
cargo test

# Specific crate
cargo test -p g3-core

# With output
cargo test -- --nocapture
```

### Test Considerations

- Provider tests may require API keys
- Computer control tests require OS permissions
- WebDriver tests require browser setup

## Do's and Don'ts for Automated Changes

### Do

- ✅ Run `cargo check` after modifications
- ✅ Run `cargo test` before committing
- ✅ Update tool definitions when adding tools
- ✅ Add tests for new functionality
- ✅ Use existing patterns for similar features
- ✅ Keep functions under 80 lines
- ✅ Update documentation for user-facing changes

### Don't

- ❌ Modify `Cargo.toml` dependencies without justification
- ❌ Add blocking code in async contexts
- ❌ Store sensitive data in plain text
- ❌ Ignore error handling
- ❌ Create deeply nested conditionals (>6 levels)
- ❌ Add external dependencies for simple tasks

## Common Incorrect Assumptions

1. **"All providers support tool calling"** - Embedded models use JSON fallback
2. **"Context window is unlimited"** - Each provider has limits (4k-200k tokens)
3. **"Tool results are always small"** - File reads can return megabytes
4. **"Sessions persist across runs"** - Sessions are ephemeral by default
5. **"All platforms are equal"** - macOS has more features (Vision, Accessibility)

## Architecture Decisions

See `DESIGN.md` for original design rationale.

Key decisions:
- **Rust for performance and safety** - Async runtime, memory safety
- **Workspace structure** - Separation of concerns, independent compilation
- **Provider abstraction** - Swap providers without code changes
- **Tool-first philosophy** - Agent acts through tools, not just advice
- **Session-scoped state** - TODO lists, logs tied to sessions

## File Structure Quick Reference

```
g3/
├── src/main.rs                    # Entry point
├── crates/
│   ├── g3-cli/src/
│   │   ├── lib.rs                 # CLI logic (~112k chars)
│   │   └── retro_tui.rs           # Retro TUI mode
│   ├── g3-core/src/
│   │   ├── lib.rs                 # Agent struct (~3400 lines)
│   │   ├── context_window.rs      # Context management
│   │   ├── tool_definitions.rs    # Tool schemas
│   │   ├── tool_dispatch.rs       # Tool routing
│   │   ├── tools/                 # Tool implementations
│   │   ├── streaming_parser.rs    # Response parsing
│   │   └── retry.rs               # Retry logic
│   ├── g3-providers/src/
│   │   ├── lib.rs                 # Provider trait
│   │   ├── anthropic.rs           # Anthropic Claude
│   │   ├── databricks.rs          # Databricks
│   │   ├── openai.rs              # OpenAI
│   │   └── embedded.rs            # Local models
│   ├── g3-config/src/lib.rs       # Configuration
│   ├── g3-planner/src/            # Planning mode
│   ├── g3-ensembles/src/          # Flock mode
│   └── g3-computer-control/src/   # Automation
├── agents/                         # Agent personas
├── docs/                           # Documentation
└── logs/                           # Session logs
```

## Pointers to Documentation

- [Architecture](docs/architecture.md) - System design and data flow
- [Configuration](docs/configuration.md) - Config file format and options
- [Tools Reference](docs/tools.md) - All available tools
- [Providers Guide](docs/providers.md) - LLM provider setup
- [Control Commands](docs/CONTROL_COMMANDS.md) - Interactive commands
- [Code Search](docs/CODE_SEARCH.md) - Tree-sitter search guide
- [Flock Mode](docs/FLOCK_MODE.md) - Multi-agent development
