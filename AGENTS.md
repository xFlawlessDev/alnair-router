### RUST ARCHITECTURE & MODULARITY RULES

1. **File Size Cap (500–800 LOC Rule):**
   - No single `.rs` file may exceed 800 lines of code.
   - When a file grows beyond this, convert `file.rs` into `file/mod.rs` (or `file.rs` + sibling submodules) and split `impl` blocks or domain logic into separate child submodules.

2. **Clean Library Roots:**
   - `lib.rs` and `main.rs` must contain ONLY module declarations (`mod foo;`), `pub use` re-exports, top-level documentation, and entrypoints.
   - Never write heavy implementation logic or global state directly inside `lib.rs`.

3. **Strict Minimum Visibility:**
   - Default to private (`fn` or `struct`).
   - Use `pub(super)` for submodule helpers shared only with immediate parent modules.
   - Use `pub(crate)` for items shared within the crate.
   - Reserve bare `pub` exclusively for items intended as part of the public crate/workspace API.

4. **The Facade Pattern (`pub use`):**
   - Keep internal file structures deep and domain-focused, but expose a shallow public API at the module root via `pub use`.

5. **Split Large `impl` Blocks:**
   - Do not write massive single `impl MyStruct` blocks.
   - Group related methods into logical submodules across multiple files using separate `impl MyStruct` blocks.

6. **Decouple via Traits:**
   - Do not tie business modules directly to concrete database repositories, network drivers, or heavy external types. Accept traits or generics (`R: TaskRepository` / `dyn TaskRepository`) to allow fast parallel compilation.

7. **Test File Hygiene:**
   - Keep inline `#[cfg(test)]` modules under 150 lines.
   - If tests exceed 150 lines, move them to a sibling `tests.rs` file within the module folder (`#[cfg(test)] mod tests;`) or into the crate-level `tests/` directory.

8. **Scoped Error Enums:**
   - Prefer scoped domain errors (e.g., `task_coordinator::Error`) over giant crate-wide error enums. Use `thiserror` for library crates.
