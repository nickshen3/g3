Prefer **obvious, readable Racket** over cleverness.

## Keep control flow clean
```racket
;; Good: match for destructuring
(match-define (list name age) (get-user-info id))

;; Good: cond over nested if
(cond
  [(empty? items) '()]
  [(special? (first items)) (handle-special items)]
  [else (process-normal items)])
```
- Use `match` / `match-define` for destructuring.
- Use `for/*` loops and sequences instead of manual recursion unless recursion is clearer.
- Use `cond`, `case`, `and`, `or` cleanly; avoid nested `if` pyramids.
- Prefer `define` over `let`/`let*` when it reduces indentation.
- Use immutable data by default; reach for mutation only when it materially improves clarity/perf.

## Modules: explicit exports
```racket
;; Good: explicit contract-out
(provide
  (contract-out
    [process-data (-> input/c output/c)]))

;; Bad: leaky exports
(provide (all-defined-out))
```
- Organize code into small modules with explicit `provide` lists.
- Put `provide` before `require` — interface at top.
- Prefer `contract-out` when exporting public APIs.
- Avoid `provide (all-defined-out)` except in quick prototypes/tests.
- Prefer `require` with explicit identifiers; avoid huge wildcard imports.
- Use `racket/base` for libraries (faster loading); use `racket` for scripts.

## Naming
- Prefix functions with data type of main argument: `board-free-spaces` not `free-spaces`.

## Contracts and types
- If in untyped Racket: add contracts at module boundaries for public APIs, especially for callbacks and data shapes.
- If the project uses `typed/racket`, keep typed/untyped boundaries clean and document them.
- Use predicates + struct definitions to make data models explicit.

## Data modeling
- Prefer `struct` (possibly `#:transparent`) for domain objects, not ad-hoc hash soup.
- For enums/variants: consider `struct` variants + `match`, or symbols with clear validation.
- Validate external data (YAML/JSON) once at the boundary; keep internal representation consistent.

## Error handling
- Use `raise-argument-error`, `raise-user-error`, or `error` with a clear message.
- Wrap IO and parsing with `with-handlers` and rethrow with context (what file, what phase).

## IO and paths
- Use `build-path`, `simplify-path`, `path->string`; don't concatenate path strings manually.
- Use `call-with-input-file` / `call-with-output-file` idiomatically.

## Performance
- Prefer vectors for hot loops / indexed access; lists for iteration; hashes for keyed lookup.
- Use `for/fold` or `for/hash` to build results efficiently.
- Use `in-list`, `in-vector`, etc. explicitly in `for` loops for better performance.
- Avoid repeated `append` in loops; accumulate then reverse if needed.

## Macros: use sparingly
- Don't write macros unless it meaningfully reduces boilerplate or enforces invariants.
- If writing macros: use `syntax-parse` (not raw `syntax-case`) and include good error messages.
- Keep macro output readable and debuggable.

## Phase separation
- Understand `for-syntax` vs runtime; don't accidentally pull runtime values into macros.
- Use `begin-for-syntax` sparingly; prefer `syntax-local-value` patterns when possible.

## Continuations: use sparingly
- Prefer `call/ec` (escape continuations) over full `call/cc` when possible.
- Use `parameterize` for dynamic scope, not continuation tricks.
- If using `parameterize`, keep scope tight and document effects.

## Concurrency
- Use `place`s for CPU parallelism, `thread`s for I/O concurrency.
- Prefer channels (`make-channel`, `channel-put`, `channel-get`) over shared state.
- Use `sync` and events for composable waiting.

## Gotchas
- `eq?` vs `equal?` vs `eqv?`: use `equal?` by default for structural comparison.
- `null?` only works on proper lists; use `empty?` from `racket/list` for generics.
- `string=?` not `equal?` for string comparison in hot paths.

## Testing
- Add `rackunit` tests for tricky logic; prefer table-driven tests.
- Use `module+ test` submodules; run with `raco test`.
- Consider Scribble docs for library boundaries.

## Size limits
- ~500 lines per module (1000 tolerable, 10000 is a god-file).
- ~66 lines per function (one screen).

## Packages/tooling
- Assume `raco fmt` style; keep formatting consistent.
- Use `raco pkg install --auto` for dependency resolution.
- Prefer `info.rkt` for package metadata over ad-hoc scripts.
