# Dependency Graph Summary

## Overview

| Metric | Value |
|--------|-------|
| Total nodes | 108 |
| Total edges | 186 |
| Crate nodes | 9 |
| File nodes | 99 |
| Crate-level edges | 14 |
| File-level edges | 172 |

## Crate Structure

| Crate | Path | Direct Dependencies |
|-------|------|---------------------|
| g3 | `.` | g3-cli, g3-providers |
| g3-cli | `crates/g3-cli` | g3-core, g3-config, g3-planner, g3-computer-control, g3-providers |
| g3-core | `crates/g3-core` | g3-providers, g3-config, g3-execution, g3-computer-control |
| g3-providers | `crates/g3-providers` | (none) |
| g3-config | `crates/g3-config` | (none) |
| g3-execution | `crates/g3-execution` | (none) |
| g3-planner | `crates/g3-planner` | g3-providers, g3-core, g3-config |
| g3-computer-control | `crates/g3-computer-control` | (none) |
| studio | `crates/studio` | (none) |

## Entrypoints

| Type | Location | Evidence |
|------|----------|----------|
| Binary | `crates/g3-cli/src/lib.rs` | `run()` function, CLI dispatch |
| Binary | `crates/studio/src/main.rs` | `main()` function |
| Library | `crates/g3-core/src/lib.rs` | `Agent` struct, core API |

## Top Fan-In Nodes (Most Depended Upon)

| Node | Fan-In | Type |
|------|--------|------|
| crate:g3-core | 35 | crate |
| crate:g3-providers | 19 | crate |
| file:crates/g3-core/src/ui_writer.rs | 15 | file |
| crate:g3-config | 13 | crate |
| file:crates/g3-cli/src/g3_status.rs | 10 | file |
| file:crates/g3-cli/src/simple_output.rs | 8 | file |
| file:crates/g3-core/src/context_window.rs | 6 | file |
| file:crates/g3-cli/src/template.rs | 6 | file |
| file:crates/g3-core/src/paths.rs | 5 | file |
| crate:g3-computer-control | 4 | crate |

## Top Fan-Out Nodes (Most Dependencies)

| Node | Fan-Out | Type |
|------|---------|------|
| file:crates/g3-cli/src/agent_mode.rs | 13 | file |
| file:crates/g3-core/src/lib.rs | 13 | file |
| file:crates/g3-cli/src/commands.rs | 12 | file |
| file:crates/g3-cli/src/interactive.rs | 12 | file |
| file:crates/g3-cli/src/accumulative.rs | 11 | file |
| file:crates/g3-planner/src/planner.rs | 8 | file |
| file:crates/g3-cli/src/autonomous.rs | 8 | file |
| file:crates/g3-core/src/tools/acd.rs | 7 | file |
| file:crates/g3-planner/src/llm.rs | 6 | file |
| file:crates/g3-cli/src/utils.rs | 5 | file |

## File Counts by Crate

| Crate | Source Files |
|-------|-------------|
| g3-cli | 23 |
| g3-core | 42 |
| g3-providers | 9 |
| g3-config | 2 |
| g3-execution | 1 |
| g3-planner | 8 |
| g3-computer-control | 11 |
| studio | 3 |

## Extraction Limitations

1. **Static analysis only**: Dynamic dispatch and trait objects not traced
2. **Use statement parsing**: Only `use g3_*` and `use crate::` patterns captured
3. **Conditional compilation**: `#[cfg(...)]` blocks not evaluated
4. **Re-exports**: `pub use` chains not fully resolved
5. **Test files excluded**: Files in `/tests/` directories not included
6. **Examples excluded**: Files in `/examples/` directories not included
