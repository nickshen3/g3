Prefer **obvious, readable Racket** over cleverness.

## Control flow
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
- Use `cond`, `case`, `and`, `or` cleanly; avoid nested `if` pyramids.
- Prefer `define` over `let`/`let*` when it reduces indentation.
- Use `let*` when bindings depend on earlier bindings; use `let` for independent bindings.

## Iteration idioms
```racket
;; Prefer for/* with explicit sequence types
(for/list ([x (in-list items)]    ; in-list for performance
           [i (in-naturals)])     ; in-naturals for indices
  (process x i))

;; for/fold for accumulation
(for/fold ([acc '()]
           [seen (set)])
          ([x (in-list items)])
  (values (cons (transform x) acc)
          (set-add seen x)))
```
- Use `for/*` loops over manual recursion unless recursion is clearer.
- Use `in-list`, `in-vector`, `in-hash`, etc. explicitly — faster than generic sequence.
- Use `for/fold` for complex accumulation; `for/list`, `for/hash` for simple transforms.
- Use `for*/list` (note the `*`) when you need nested iteration flattened.

## Data structure mutability
- **Immutable by default**: lists, immutable hashes, immutable vectors for most code.
- **Mutable when**: you need O(1) update in a hot loop, or modeling inherently stateful things.
- **Mutable hashes** (`make-hash`): use for caches, memoization, symbol tables.
- **Mutable vectors** (`make-vector`): use for fixed-size buffers, matrix ops.
- **Boxes** (`box`, `unbox`, `set-box!`): use for single mutable cells, rarely needed.
- Don't mix: if a data structure is mutable, keep it internal; expose immutable views.

## Performance
- Use `in-list`, `in-vector`, `in-hash` explicitly in `for` loops — faster than generic sequence.
- **Beware `list-ref` in a loop** — it's O(n) per call, so O(n²) overall. Use vectors for indexed access.
- Don't repeatedly `append` in loops; use `for/list` or accumulate with `cons` then `reverse`.
- Prefer vectors for indexed access, hashes for keyed lookup, lists for sequential iteration.
- Use `for/fold` to build results in one pass instead of multiple traversals.

## Module hygiene
```racket
;; Good: explicit contract-out, interface at top
(provide
  (contract-out
    [process-data (-> input/c output/c)]
    [make-processor (-> config/c processor/c)]))

(require racket/match
         "internal-utils.rkt")
```
- **One abstraction per module** (~500 lines rule of thumb).
- Put `provide` before `require` — interface at top.
- Use `contract-out` when correctness matters (public APIs, callbacks, data shapes).
- Use explicit `provide` lists only — never `(all-defined-out)` in production.
- Use `racket/base` for libraries (faster loading); `racket` for scripts.

## Parameters and dynamic scope
- **Good uses**: current ports, logging context, configuration, test fixtures.
- **Bad uses**: hidden global state that affects correctness, implicit arguments to avoid passing data.
- Keep `parameterize` scope tight — wrap the smallest expression that needs it.
- Document when a function reads from a parameter (it's implicit input).
- Prefer explicit arguments over parameters when the caller should always think about the value.

## Contracts: when and how much
- **Module boundaries**: use `contract-out` for public APIs — catches bugs at the boundary with clear blame.
- **Internal functions**: use `define/contract` sparingly for tricky invariants or during debugging.
- **Higher-order contracts**: use `->` for simple functions; `->i` when you need dependent contracts.
- **In tests**: contracts give fast feedback — keep them on during development, consider `#:unprotected-submodule` for perf-critical production paths.
- **Don't go nuts**: contracts at every internal function add overhead and noise. Focus on boundaries.

## Naming
- Prefix functions with data type of main argument: `board-ref`, `board-free-spaces`, not `ref`, `free-spaces`.
- Use `-ref`, `-set`, `-update` suffixes for accessors/mutators on custom types.
- Avoid abbreviations except well-known ones (`idx`, `len`, `ctx`).

## Data modeling
- Prefer `struct` (possibly `#:transparent`) for domain objects, not ad-hoc hash soup.
- For enums/variants: `struct` variants + `match`, or symbols with clear validation.
- Validate external data (YAML/JSON) once at the boundary; keep internal representation consistent.

## Error handling
- Use `raise-argument-error`, `raise-user-error`, or `error` with a clear message.
- Wrap IO and parsing with `with-handlers` and rethrow with context (what file, what phase).

## IO and paths
- Use `build-path`, `simplify-path`, `path->string`; don't concatenate path strings manually.
- Use `call-with-input-file` / `call-with-output-file` idiomatically.

## Macros: use sparingly
- Don't write macros unless it meaningfully reduces boilerplate or enforces invariants.
- If writing macros: use `syntax-parse` (not raw `syntax-case`) and include good error messages.
- Keep macro output readable and debuggable.

## Phase separation
- Understand `for-syntax` vs runtime; don't accidentally pull runtime values into macros.
- Use `begin-for-syntax` sparingly; prefer `syntax-local-value` patterns when possible.

## Continuations
- Prefer `call/ec` (escape continuations) over full `call/cc` — simpler, faster, sufficient for early exit.
- Don't use continuations for what `parameterize` or exceptions handle better.

## Concurrency
- Use `place`s for CPU parallelism, `thread`s for I/O concurrency.
- Prefer channels (`make-channel`, `channel-put`, `channel-get`) over shared state.
- Use `sync` and events for composable waiting.

## Gotchas
- `eq?` vs `equal?` vs `eqv?`: use `equal?` by default for structural comparison.
- `null?` only works on proper lists; use `empty?` from `racket/list` for generics.
- `string=?` not `equal?` for string comparison in hot paths.

## Testing
- Use `module+ test` submodules; run with `raco test`.
- Add `rackunit` tests for tricky logic; prefer table-driven `test-case` with `check-equal?`.
- Consider Scribble docs for library boundaries.

## Size heuristics
- **One abstraction per module** — if you're documenting two unrelated things, split.
- **One screen per function** (~66 lines) — if you can't see the whole function, extract helpers.
