RACKET-SPECIFIC GUIDANCE (apply by default)

- Prefer idiomatic Racket:
  - Use `match` / `match-define` for destructuring.
  - Use `for/*` loops and sequences instead of manual recursion unless recursion is clearer.
  - Use `cond`, `case`, `and`, `or` cleanly; avoid nested `if` pyramids.
  - Use immutable data by default; reach for mutation only when it materially improves clarity/perf.

- Modules and structure:
  - Organize code into small modules with explicit `provide` lists (prefer `provide (contract-out ...)` when exporting).
  - Avoid `provide (all-defined-out)` except in quick prototypes/tests.
  - Prefer `require` with explicit identifiers; avoid huge wildcard imports.

- Contracts and types:
  - If in untyped Racket: add contracts at module boundaries for public APIs (`contract-out`), especially for callbacks and data shapes.
  - If the project uses `typed/racket`, keep typed/untyped boundaries clean and document them.
  - Use predicates + struct definitions to make data models explicit.

- Data modeling:
  - Prefer `struct` (possibly `#:transparent`) for domain objects, not ad-hoc hash soup.
  - For enums/variants: consider `struct` variants + `match`, or symbols with clear validation.
  - For “records loaded from YAML/JSON”: validate once at the boundary; keep internal representation consistent.

- Error handling:
  - Use `raise-argument-error`, `raise-user-error`, or `error` with a clear message.
  - Wrap IO and parsing with `with-handlers` and rethrow with context (what file, what phase).
  - Don’t swallow exceptions; surface actionable diagnostics.

- IO, paths, and portability:
  - Use `build-path`, `simplify-path`, `path->string` as needed; don’t concatenate path strings manually.
  - Use `call-with-input-file` / `call-with-output-file` and ports idiomatically.
  - Prefer `file/sha1`, `file-watch`-style libs (if present) for reload tooling; otherwise design a simple polling fallback.

- Performance + allocations:
  - Prefer vectors for hot loops / indexed access; lists for iteration; hashes for keyed lookup.
  - Use `for/fold` or `for/hash` to build results efficiently.
  - Avoid repeated `append` in loops; accumulate then reverse if needed.
  - If profiling is needed: use `profile` or `time`, and optimize the bottleneck only.

- Macros and syntax:
  - Don’t write macros unless it meaningfully reduces boilerplate or enforces invariants.
  - If writing macros: use `syntax-parse` (not raw `syntax-case`) and include good error messages.
  - Keep macro output readable and debuggable.

- Testing + docs:
  - Add `rackunit` tests for tricky logic; prefer table-driven tests.
  - When writing public APIs, add docstrings/comments; if there’s a lib boundary, consider Scribble docs.
  - Include runnable examples in comments when it helps.

- Concurrency/events (common in engines/tools):
  - Prefer clear event loops and message passing; avoid shared mutable state unless protected.
  - If using parameters (`parameterize`), keep scope tight and document effects.

- Packages/tooling:
  - Assume `raco fmt` / `racket-format` style; keep formatting consistent.
  - If suggesting deps, name the package and `raco pkg install` usage.

- Output expectations:
  - When proposing code changes, include: new/changed function signatures, required `require`s, and small usage examples.
  - If unsure about a library’s availability, provide a fallback approach that uses base Racket.