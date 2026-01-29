# Coupling Hotspots

## High Fan-In Files (Most Depended Upon)

Files with disproportionately high incoming dependencies.

| File | Fan-In | Crate | Role |
|------|--------|-------|------|
| ui_writer.rs | 15 | g3-core | UI abstraction trait |
| g3_status.rs | 10 | g3-cli | Status message formatting |
| simple_output.rs | 8 | g3-cli | Output helper |
| context_window.rs | 6 | g3-core | Token/context management |
| template.rs | 6 | g3-cli | Template processing |
| paths.rs | 5 | g3-core | Path utilities |

### Evidence: ui_writer.rs (fan-in: 15)

Depended on by:
- g3-cli: accumulative.rs, agent_mode.rs, autonomous.rs, commands.rs, interactive.rs, task_execution.rs, ui_writer_impl.rs, utils.rs
- g3-core: compaction.rs, feedback_extraction.rs, lib.rs, retry.rs, tool_dispatch.rs, tools/*.rs

### Evidence: g3_status.rs (fan-in: 10)

Depended on by:
- commands.rs, interactive.rs, simple_output.rs, task_execution.rs, and others in g3-cli

## High Fan-Out Files (Most Dependencies)

Files with disproportionately high outgoing dependencies.

| File | Fan-Out | Crate | Role |
|------|---------|-------|------|
| agent_mode.rs | 13 | g3-cli | Agent mode entry point |
| lib.rs | 13 | g3-core | Core library root |
| commands.rs | 12 | g3-cli | Command handlers |
| interactive.rs | 12 | g3-cli | Interactive REPL |
| accumulative.rs | 11 | g3-cli | Accumulative mode |
| planner.rs | 8 | g3-planner | Planning orchestration |

### Evidence: agent_mode.rs (fan-out: 13)

Depends on:
- g3-core: Agent, ui_writer::UiWriter
- Internal: project_files, display, language_prompts, simple_output, embedded_agents, ui_writer_impl, interactive, template

### Evidence: g3-core/lib.rs (fan-out: 13)

Depends on:
- External crates: g3-config, g3-providers
- Internal modules: ui_writer, context_window, paths, compaction, streaming, tools, etc.

## Cross-Crate Coupling

| Source Crate | Target Crate | Edge Count |
|--------------|--------------|------------|
| g3-cli | g3-core | 35 |
| g3-core | g3-providers | 10 |
| g3-core | g3-config | 5 |
| g3-planner | g3-core | 4 |
| g3-planner | g3-providers | 3 |
| g3-core | g3-computer-control | 2 |

## Observations

1. **ui_writer.rs** is a central abstraction point; changes here affect 15+ files
2. **g3-cli** files have high fan-out due to orchestration responsibilities
3. **g3-core/lib.rs** is the primary API surface with expected high coupling
4. **g3_status.rs** and **simple_output.rs** form a UI utility cluster in g3-cli
5. **tools/*.rs** files consistently depend on ui_writer and ToolCall types
