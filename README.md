# g3 - AI Coding Agent

g3 is a coding AI agent designed to help you complete tasks by writing code and executing commands. Built in Rust, it provides a flexible architecture for interacting with various Large Language Model (LLM) providers while offering powerful code generation and task automation capabilities.

## Architecture Overview

g3 follows a modular architecture organized as a Rust workspace with multiple crates, each responsible for specific functionality:

### Core Components

#### **g3-core**
The heart of the agent system, containing:
- **Agent Engine**: Main orchestration logic for handling conversations, tool execution, and task management
- **Context Window Management**: Intelligent tracking of token usage with context thinning (50-80%) and auto-compaction at 80% capacity
- **Tool System**: Built-in tools for file operations, shell commands, computer control, TODO management, and structured output
- **Streaming Response Parser**: Real-time parsing of LLM responses with tool call detection and execution
- **Task Execution**: Support for single and iterative task execution with automatic retry logic

#### **g3-providers**
Abstraction layer for LLM providers:
- **Provider Interface**: Common trait-based API for different LLM backends
- **Multiple Provider Support**: 
  - Anthropic (Claude models)
  - Databricks (DBRX and other models)
  - Local/embedded models via llama.cpp with Metal acceleration on macOS
- **OAuth Authentication**: Built-in OAuth flow support for secure provider authentication
- **Provider Registry**: Dynamic provider management and selection

#### **g3-config**
Configuration management system:
- Environment-based configuration
- Provider credentials and settings
- Model selection and parameters
- Runtime configuration options

#### **g3-execution**
Task execution framework:
- Task planning and decomposition
- Execution strategies (sequential, parallel)
- Error handling and retry mechanisms
- Progress tracking and reporting

#### **g3-computer-control**
Computer control capabilities:
- Mouse and keyboard automation
- UI element inspection and interaction
- Screenshot capture and window management
- OCR text extraction via Tesseract

#### **g3-cli**
Command-line interface:
- Interactive terminal interface
- Task submission and monitoring
- Configuration management commands
- Session management

### Error Handling & Resilience

g3 includes robust error handling with automatic retry logic:
- **Recoverable Error Detection**: Automatically identifies recoverable errors (rate limits, network issues, server errors, timeouts)
- **Exponential Backoff with Jitter**: Implements intelligent retry delays to avoid overwhelming services
- **Detailed Error Logging**: Captures comprehensive error context including stack traces, request/response data, and session information
- **Error Persistence**: Saves detailed error logs to `.g3/errors/` for post-mortem analysis
- **Graceful Degradation**: Non-recoverable errors are logged with full context before terminating

### Tool Call Duplicate Detection

g3 includes intelligent duplicate detection to prevent the LLM from accidentally calling the same tool twice in a row:
- **Sequential Duplicate Prevention**: Only immediately sequential identical tool calls are blocked
- **Text Separation Allowed**: If there's any text between tool calls, they're not considered duplicates
- **Session-Wide Reuse**: Tools can be called multiple times throughout a session - only back-to-back duplicates are prevented

This catches cases where the LLM "stutters" and outputs the same tool call twice, while still allowing legitimate re-use of tools.

### Timing Footer

After each response, g3 displays a timing footer showing elapsed time, time to first token, token usage (from the LLM, not estimated), and current context window usage percentage. The token and context info is displayed dimmed for a clean interface.

## Key Features

### Intelligent Context Management
- Automatic context window monitoring with percentage-based tracking
- Smart auto-compaction when approaching token limits
- **Context thinning** at 50%, 60%, 70%, 80% thresholds - automatically replaces large tool results with file references
- Conversation history preservation through summaries
- Dynamic token allocation for different providers (4k to 200k+ tokens)

### Interactive Control Commands
g3's interactive CLI includes control commands for manual context management:
- **`/compact`**: Manually trigger compaction to compact conversation history
- **`/thinnify`**: Manually trigger context thinning to replace large tool results with file references
- **`/skinnify`**: Manually trigger full context thinning (like `/thinnify` but processes the entire context window, not just the first third)
- **`/readme`**: Reload README.md and AGENTS.md from disk without restarting
- **`/stats`**: Show detailed context and performance statistics
- **`/help`**: Display all available control commands

These commands give you fine-grained control over context management, allowing you to proactively optimize token usage and refresh project documentation. See [Control Commands Documentation](docs/CONTROL_COMMANDS.md) for detailed usage.

### Tool Ecosystem
- **File Operations**: Read, write, and edit files with line-range precision
- **Shell Integration**: Execute system commands with output capture
- **Code Generation**: Structured code generation with syntax awareness
- **TODO Management**: Read and write TODO lists with markdown checkbox format
- **Computer Control** (Experimental): Automate desktop applications
  - Mouse and keyboard control
  - UI element inspection
  - Screenshot capture and window management
  - Window listing and identification
- **Code Search**: Embedded tree-sitter for syntax-aware code search (Rust, Python, JavaScript, TypeScript, Go, Java, C, C++) - see [Code Search Guide](docs/CODE_SEARCH.md)
- **Final Output**: Formatted result presentation

### Provider Flexibility
- Support for multiple LLM providers through a unified interface
- Hot-swappable providers without code changes
- Provider-specific optimizations and feature support
- Local model support for offline operation

### Embedded Models (Local LLMs)

g3 supports local models via llama.cpp with Metal acceleration on macOS. Here's a performance comparison for **agentic tasks** (multi-step tool-calling workflows):

**Test case**: Comic book repacking - extract CBR/CBZ archives, reorder files preserving page and issue order, repack into single archive. Requires correct sequencing, file handling, and no race conditions.

#### Cloud Models (Baseline)
| Model | Agentic Score | Notes |
|-------|---------------|-------|
| **Claude Opus 4.5** | ⭐⭐⭐⭐⭐ | Flawless execution |
| Claude Sonnet 4.5 | ⭐⭐⭐⭐ | Good, occasional issues |
| Claude 4 family | ⭐⭐⭐ | Gets there eventually, needs manual checking |

#### Local Models
| Model | Size | Speed | Agentic Score | Notes |
|-------|------|-------|---------------|-------|
| ~~Qwen3-32B~~ (Dense) | 18 GB | Slow | ❌ | Good reasoning, but flails on execution and crashes |
| Qwen3-14B | 8.4 GB | Medium | ⭐⭐ | Understands tasks but makes implementation errors |
| GLM-4 9B | 5.7 GB | Fast | ⭐⭐ | Quick responses, may struggle with complex tasks |
| Qwen3-4B | 2.3 GB | Very Fast | ❌ | Generates malformed tool calls - not for agentic use |
| ~~Qwen3-30B-A3B~~ (MoE) | 17 GB | Very Fast | ❌ | **Avoid** - loops infinitely on tool calls |

**Key findings**:
- **Dense models** (Qwen3-32B, Qwen3-14B) handle agentic loops correctly
- **MoE models** (Qwen3-30B-A3B) are fast but don't know when to stop tool-calling
- **Metal GPU** works well with dense models on Apple Silicon
- Even the best local models (32B) lag significantly behind Claude Opus 4.5 on complex tasks
- Local models are best for simpler agentic tasks or when offline/privacy is required

Configuration example:
```toml
[providers.embedded.qwen3-big]
model_path = "~/.g3/models/Qwen_Qwen3-32B-Q4_K_M.gguf"
model_type = "qwen"
context_length = 40960
gpu_layers = 99  # Full GPU offload on Apple Silicon
```

### Task Automation
- Single-shot task execution for quick operations
- Iterative task mode for complex, multi-step workflows
- Automatic error recovery and retry logic
- Progress tracking and intermediate result handling

## Language & Technology Stack

- **Language**: Rust (2021 edition)
- **Async Runtime**: Tokio for concurrent operations
- **HTTP Client**: Reqwest for API communications
- **Serialization**: Serde for JSON handling
- **CLI Framework**: Clap for command-line parsing
- **Logging**: Tracing for structured logging (INFO logs converted to DEBUG for cleaner CLI output)
- **Local Models**: llama.cpp with Metal acceleration support

## Use Cases

g3 is designed for:
- Automated code generation and refactoring
- File manipulation and project scaffolding
- System administration tasks
- Data processing and transformation  
- API integration and testing
- Documentation generation
- Complex multi-step workflows
- Parallel development of modular architectures
- Desktop application automation and testing

## Getting Started

### Default Mode: Accumulative Autonomous

The default interactive mode now uses **accumulative autonomous mode**, which combines the best of interactive and autonomous workflows:

```bash
# Simply run g3 in any directory
g3

# You'll be prompted to describe what you want to build
# Each input you provide:
# 1. Gets added to accumulated requirements
# 2. Automatically triggers autonomous mode (coach-player loop)
# 3. Implements your requirements iteratively

# Example session:
requirement> create a simple web server in Python with Flask
# ... autonomous mode runs and implements it ...
requirement> add a /health endpoint that returns JSON
# ... autonomous mode runs again with both requirements ...
```

### Other Modes

```bash
# Single-shot mode (one task, then exit)
g3 "implement a function to calculate fibonacci numbers"

# Traditional autonomous mode (reads requirements.md)
g3 --autonomous

# Traditional chat mode (simple interactive chat without autonomous runs)
g3 --chat
```

### Planning Mode

Planning mode provides a structured workflow for requirements-driven development with git integration:

```bash
# Start planning mode for a codebase
g3 --planning --codepath ~/my-project --workspace ~/g3_workspace

# Without git operations (for repos not yet initialized)
g3 --planning --codepath ~/my-project --no-git --workspace ~/g3_workspace
```

Planning mode workflow:
1. **Refine Requirements**: Write requirements in `<codepath>/g3-plan/new_requirements.md`, then let the LLM suggest improvements
2. **Implement**: Once requirements are approved, they're renamed to `current_requirements.md` and the coach/player loop implements them
3. **Complete**: After implementation, files are archived with timestamps (e.g., `completed_requirements_2025-01-15_10-30-00.md`)
4. **Git Commit**: Staged files are committed with an LLM-generated commit message
5. **Repeat**: Return to step 1 for the next iteration

All planning artifacts are stored in `<codepath>/g3-plan/`:
- `planner_history.txt` - Audit log of all planning activities
- `new_requirements.md` / `current_requirements.md` - Active requirements
- `todo.g3.md` - Implementation TODO list
- `completed_*.md` - Archived requirements and todos

See the configuration section for setting up different providers for the planner role.

```bash
# Build the project
cargo build --release

# Run from the build directory
./target/release/g3

# Or copy both files to somewhere in your PATH (macOS only needs both files)
cp target/release/g3 ~/.local/bin/
cp target/release/libVisionBridge.dylib ~/.local/bin/  # macOS only

# Execute a task
g3 "implement a function to calculate fibonacci numbers"
```

## Configuration

G3 uses a TOML configuration file for settings. The config file is automatically created at `~/.config/g3/config.toml` on first run with sensible defaults.

### Retry Configuration

g3 includes configurable retry logic for handling recoverable errors (timeouts, rate limits, network issues, server errors):

```toml
[agent]
max_context_length = 8192
enable_streaming = true
timeout_seconds = 60

# Retry configuration for recoverable errors
max_retry_attempts = 3              # Default mode retry attempts
autonomous_max_retry_attempts = 6   # Autonomous mode retry attempts
```

**Retry Behavior:**
- **Default Mode** (`max_retry_attempts`): Used for interactive chat and single-shot tasks. Default: 3 attempts.
- **Autonomous Mode** (`autonomous_max_retry_attempts`): Used for long-running autonomous tasks. Default: 6 attempts.
- Retries use exponential backoff with jitter to avoid overwhelming services
- Autonomous mode spreads retries over ~10 minutes to handle extended outages
- Only recoverable errors are retried (timeouts, rate limits, 5xx errors, network issues)
- Non-recoverable errors (auth failures, invalid requests) fail immediately

**Example:** To increase timeout resilience in autonomous mode, set `autonomous_max_retry_attempts = 10` in your config.

See `config.example.toml` for a complete configuration example.

## WebDriver Browser Automation

g3 includes WebDriver support for browser automation tasks. Chrome headless is the default, with Safari available as an alternative.

**One-Time Setup** (macOS only):

If you want to use Safari instead of Chrome headless, Safari Remote Automation must be enabled. Run this once:

```bash
# Option 1: Use the provided script
./scripts/enable-safari-automation.sh

# Option 2: Enable manually
safaridriver --enable  # Requires password

# Option 3: Enable via Safari UI
# Safari → Preferences → Advanced → Show Develop menu
# Then: Develop → Allow Remote Automation
```

**Usage**:

```bash
# Use Safari (opens a visible browser window)
g3 --safari

# Use Chrome in headless mode (default, no visible window, runs in background)
g3
```

**Chrome Setup Options**:

*Option 1: Use Chrome for Testing (Recommended)* - Guarantees version compatibility:
```bash
./scripts/setup-chrome-for-testing.sh
```
Then add to your `~/.config/g3/config.toml`:
```toml
[webdriver]
chrome_binary = "/Users/yourname/.chrome-for-testing/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
```

*Option 2: Use system Chrome* - Requires matching ChromeDriver version:
- macOS: `brew install chromedriver`
- Linux: `apt install chromium-chromedriver`
- Or download from: https://chromedriver.chromium.org/downloads

**Note**: If you see "ChromeDriver version doesn't match Chrome version" errors, use Option 1 (Chrome for Testing) which bundles matching versions.

## Computer Control (Experimental)

g3 can interact with your computer's GUI for automation tasks:

**Available Tools**: `mouse_click`, `type_text`, `find_element`, `take_screenshot`, `list_windows`

**Setup**: Enable in config with `computer_control.enabled = true` and grant OS accessibility permissions:
- **macOS**: System Preferences → Security & Privacy → Accessibility  
- **Linux**: Ensure X11 or Wayland access
- **Windows**: Run as administrator (first time only)

## Session Logs

G3 automatically saves session logs for each interaction in the `.g3/sessions/` directory. These logs contain:
- Complete conversation history
- Token usage statistics
- Timestamps and session status

The `.g3/` directory is created automatically on first use and is excluded from version control.

## Agent Mode

Agent mode runs specialized AI agents with custom prompts tailored for specific tasks. Each agent has a distinct personality and focus area.

### Built-in Agents

g3 comes with several embedded agents that work out of the box:

| Agent | Focus |
|-------|-------|
| **carmack** | Code readability and craft - simplifies, refactors, improves naming |
| **hopper** | Testing and quality - writes tests, finds edge cases |
| **euler** | Architecture and dependencies - analyzes structure, finds coupling |
| **huffman** | Memory maintenance - compacts, deduplicates, increases signal |
| **lamport** | Concurrency and correctness - reviews async code, finds race conditions |
| **fowler** | Refactoring patterns - applies design patterns, reduces duplication |
| **breaker** | Adversarial testing - finds bugs, creates minimal repros |
| **scout** | Research - investigates APIs, libraries, approaches |

### Usage

```bash
# List all available agents
g3 --list-agents

# Run an agent on the current project
g3 --agent carmack

# Run an agent with a specific task
g3 --agent hopper "add tests for the parser module"
```

### Custom Agents

Create custom agents by adding markdown files to `agents/<name>.md` in your workspace. Workspace agents override embedded agents with the same name, allowing per-project customization.

## Studio - Multi-Agent Workspace Manager

Studio is a companion tool for managing multiple g3 agent sessions using git worktrees. Each session runs in an isolated worktree with its own branch, allowing multiple agents to work on the same codebase without conflicts.

### Usage

```bash
# Build studio alongside g3
cargo build --release

# Run an agent session (creates worktree, runs g3, tails output)
studio run --agent carmack "fix the memory leak in cache.rs"

# Run a one-shot session without a specific agent
studio run "add unit tests for the parser module"

# List all sessions
studio list

# Check session status (shows summary when complete)
studio status <session-id>

# Accept a session: merge changes to main and cleanup
studio accept <session-id>

# Discard a session: delete without merging
studio discard <session-id>
```

### How It Works

1. **Isolation**: Each session creates a git worktree at `.worktrees/sessions/<agent>/<session-id>/`
2. **Branching**: Sessions run on branches named `sessions/<agent>/<session-id>`
3. **Tracking**: Session metadata is stored in `.worktrees/.sessions/`
4. **Workflow**: Run → Review → Accept (merge) or Discard (delete)

Studio is the recommended way to run multiple agents in parallel on the same codebase, replacing the deprecated flock mode.

## Documentation Map

Detailed documentation is available in the `docs/` directory:

| Document | Description |
|----------|-------------|
| [Architecture](docs/architecture.md) | System design, crate responsibilities, data flow |
| [Configuration](docs/configuration.md) | Config file format, provider setup, all options |
| [Tools Reference](docs/tools.md) | Complete reference for all available tools |
| [Providers Guide](docs/providers.md) | LLM provider setup and selection guide |
| [Control Commands](docs/CONTROL_COMMANDS.md) | Interactive `/` commands for context management |
| [Code Search](docs/CODE_SEARCH.md) | Tree-sitter code search query patterns |

For AI agents working with this codebase, see [AGENTS.md](AGENTS.md).

Additional resources:
- `DESIGN.md` - Original design document and rationale
- `config.example.toml` - Complete configuration example
- `config.coach-player.example.toml` - Multi-role configuration example

## License

MIT License
