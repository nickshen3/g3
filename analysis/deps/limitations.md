# Analysis Limitations

## What Was Observed

| Category | Method | Coverage |
|----------|--------|----------|
| Crate dependencies | Cargo.toml parsing | Complete |
| Module structure | `mod` declarations in lib.rs/main.rs | Complete |
| File imports | `use g3_*` and `use crate::` patterns | Partial |
| Submodule structure | mod.rs file inspection | Complete |

## What Could Not Be Observed

### 1. Implicit Dependencies

- **Trait implementations**: A file implementing a trait from another module creates a dependency not captured by `use` statements
- **Type aliases**: Types re-exported through `pub use` chains
- **Derive macros**: Dependencies introduced by `#[derive(...)]`

### 2. Dynamic Patterns

- **Trait objects**: `dyn Trait` usage creates runtime dependencies
- **Generic bounds**: `T: SomeTrait` constraints
- **Associated types**: Dependencies through associated type resolution

### 3. Conditional Compilation

- **Platform-specific code**: `#[cfg(target_os = "...")]` blocks not evaluated
- **Feature flags**: `#[cfg(feature = "...")]` dependencies not traced
- **Test-only code**: `#[cfg(test)]` modules excluded

### 4. Build System

- **build.rs**: Build script dependencies not analyzed
- **Procedural macros**: Macro crate dependencies not traced
- **Workspace-level features**: Feature unification effects not modeled

### 5. Excluded Files

| Category | Count | Reason |
|----------|-------|--------|
| Test files (`/tests/`) | ~40 | Out of scope |
| Example files (`/examples/`) | ~10 | Out of scope |
| Worktree copies | ~100 | Duplicates |

## What Was Inferred

| Inference | Basis | Confidence |
|-----------|-------|------------|
| Layer assignment | Topological sort of crate deps | High |
| Fan-in/fan-out metrics | Edge counting | High |
| Module-to-file mapping | Naming convention (mod.rs, *.rs) | High |
| Entrypoints | lib.rs/main.rs presence | High |

## What May Invalidate Conclusions

1. **Hidden re-exports**: If `g3-core/lib.rs` re-exports types from submodules, actual coupling may be higher
2. **Macro-generated code**: Macros like `#[derive(Serialize)]` add implicit serde dependency
3. **Runtime plugin loading**: If any crate loads code dynamically, static analysis misses it
4. **Workspace member changes**: Adding/removing crates from workspace invalidates crate-level graph

## Recommendations for Improved Analysis

1. Use `cargo metadata` for authoritative crate graph
2. Use `cargo tree` for transitive dependency analysis
3. Use `rust-analyzer` for semantic import resolution
4. Parse AST for type references beyond `use` statements
