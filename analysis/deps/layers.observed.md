# Observed Layering

## Crate-Level Layers

Based on dependency direction, the following layers are observed:

```
Layer 0 (Leaf crates - no internal dependencies):
  ├── g3-config
  ├── g3-execution  
  ├── g3-providers
  ├── g3-computer-control
  └── studio

Layer 1 (Depends on Layer 0):
  └── g3-core
        └── depends on: g3-providers, g3-config, g3-execution, g3-computer-control

Layer 2 (Depends on Layer 0 and 1):
  ├── g3-cli
  │     └── depends on: g3-core, g3-config, g3-planner, g3-computer-control, g3-providers
  └── g3-planner
        └── depends on: g3-providers, g3-core, g3-config

Layer 3 (Application root):
  └── g3
        └── depends on: g3-cli, g3-providers
```

## Layer Metrics

| Layer | Crates | Description |
|-------|--------|-------------|
| 0 | 5 | Foundation crates with no internal deps |
| 1 | 1 | Core engine |
| 2 | 2 | High-level orchestration |
| 3 | 1 | Application entry point |

## Observed Groupings by Path

### g3-core/src/tools/
Tool implementations grouped under `tools/` submodule:
- executor.rs (tool context and execution)
- acd.rs, file_ops.rs, memory.rs, misc.rs, research.rs, shell.rs, todo.rs, webdriver.rs

### g3-core/src/code_search/
Code search functionality:
- mod.rs (API types)
- searcher.rs (tree-sitter implementation)

### g3-computer-control/src/platform/
Platform-specific implementations:
- macos.rs, linux.rs, windows.rs (conditional compilation)

### g3-computer-control/src/webdriver/
Browser automation:
- safari.rs, chrome.rs, diagnostics.rs

### g3-providers/src/
LLM provider implementations:
- anthropic.rs, openai.rs, gemini.rs, databricks.rs, embedded.rs
- oauth.rs (authentication)
- mock.rs (testing)

## Directionality

| From | To | Direction | Violations |
|------|----|-----------|------------|
| g3 | g3-cli | down | none |
| g3-cli | g3-core | down | none |
| g3-cli | g3-planner | lateral | none |
| g3-core | g3-providers | down | none |
| g3-core | g3-config | down | none |
| g3-planner | g3-core | lateral | none |

## Uncertainty

- **studio**: Isolated crate with no detected internal dependencies; may have runtime integration not captured
- **g3-execution**: Minimal crate (1 file); purpose unclear from static analysis alone
- **Lateral dependencies**: g3-cli ↔ g3-planner relationship suggests potential for extraction or consolidation
