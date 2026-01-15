Prefer **obvious, readable Rust** over cleverness.

## Keep control flow shallow
```rust
let Some(user) = get_user(id) else {
    return Err(anyhow!("user not found"));
};
```
- Prefer early returns (`return`, `?`) over deep nesting.
- Use `let ... else { ... }` to reduce indentation.
- Prefer `match` when it clarifies exhaustiveness; prefer `if let` for single-arm cases.

## Name things for humans
- Use concrete, intention-revealing names:
  - `bytes`, `chars`, `graphemes` (don't call everything `data`)
  - `request`, `response`, `state`, `config`, `opts`
  - `*_path`, `*_dir`, `*_id`, `*_idx` (be explicit about units)
- If something is a char index, `*_char_idx`, etc.

## Make invariants explicit in types when cheap
- Prefer newtypes for confusing primitives (`UserId`, `SessionId`) when it reduces mistakes.
- Prefer enums over magic strings/ints for state.

## Prefer small helper functions over mega-blocks
- Extract helpers when a block:
  - has >1 responsibility
  - has nested matches/loops
  - repeats subtle conditions
- Helpers should have crisp names and minimal parameter lists.

## Error handling: clarity > cleverness
- Avoid `unwrap()` / `expect()` in production paths.
- Use `anyhow`/`thiserror` patterns already present in the repo; don't introduce new error stacks casually.
- Add context where it matters (`.context("...")`) but don't spray context everywhere.

## Iterators vs loops
- Prefer a `for` loop when it's clearer than an iterator chain.
- If you need a comment to explain an iterator chain, use a loop instead.
- Use `collect::<Result<Vec<_>, _>>()?` when it's idiomatic *AND* readable; otherwise a loop with `push` is fine.

## Strings are UTF-8: do not do byte slicing
- Never slice `String`/`&str` using byte indices (`s[a..b]`) unless you can prove ASCII.
- Prefer:
  - `char_indices()` for char-aware operations
  - `unicode-segmentation` graphemes when user-perceived characters matter
  - helper functions in-repo (if present) for safe truncation/slicing

## Ownership/lifetimes: avoid "lifetime gymnastics"
- Prefer owned values at module boundaries if lifetimes complicate readability.
- Avoid returning references tied to complex internal state unless there's a strong perf reason.

## Async: don't block the runtime
- Never call blocking I/O (`std::fs`, `std::net`) in async functions without `spawn_blocking`.
- Prefer `tokio::fs` over `std::fs` in async contexts.
- Keep futures `Send` unless you have a specific reason not to.

## Visibility: minimize pub surface
- Prefer `pub(crate)` over `pub` for internal APIs.
- Keep struct fields private with accessor methods when invariants matter.

## Generics: prefer simplicity
- Prefer `fn process(items: impl Iterator<Item = T>)` over `fn process<I: Iterator<Item = T>>(items: I)` when there's only one generic parameter.
- Avoid trait bounds that require reading three lines of `where` clauses.

## What *NOT* to do
- Do not introduce macros or advanced patterns (typestate, proc-macros, async trait hacks) unless the repo already uses them and it clearly improves readability.
- Do not add `dbg!()` calls in committed code.
- Do not create "god files"—split by responsibility when a module grows unwieldy.
