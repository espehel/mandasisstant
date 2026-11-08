# Agent instructions

## Main priority: clean, minimal solutions

Implement the smallest clear solution that fully satisfies the current request. Optimize for code that is easy to read, change, and delete. Fewer concepts and moving parts matter more than fewer lines.

- Implement required behavior only. Do not add speculative features, configuration, abstractions, dependencies, or fallback systems.
- Keep changes focused. Preserve unrelated work and avoid incidental refactors, formatting churn, and dependency upgrades.
- Prefer direct functions, explicit data flow, and existing platform APIs. Introduce an abstraction when it simplifies an actual requirement or repeated behavior, not an imagined future use case.
- A little straightforward duplication is preferable to a premature generic framework. Avoid service layers, factories, wrapper hooks, and shared utilities that only forward calls without adding meaning.
- Correctness, necessary error handling, accessibility, and safe native access are part of a complete solution; do not defer them merely to keep a diff small.

## Scope and optional work

- Make routine implementation decisions independently within the requested scope.
- Leave optional improvements unimplemented. For a concrete, relevant follow-up, add a short `TODO` beside the affected code explaining what is deferred and when it becomes necessary.
- Discuss optional work with the developer when it changes product behavior, architecture, dependencies, permissions, or persistent data. Explain the benefit and cost before implementing it.
- Do not create placeholder interfaces, unused options, commented-out implementations, or speculative TODO lists.
- Never use a TODO to leave required behavior incomplete or knowingly hide a correctness or security problem introduced by the change.

Example of an optional follow-up:

```ts
// TODO: Add batch translation if the developer requests multi-item input.
```

## Repository context

- Frontend: React 19, strict TypeScript, Vite, and plain CSS in `src/`.
- Native application: Tauri 2 and Rust in `src-tauri/`.
- Apple integration: Swift package in `AppleBridge/`, connected through `swift-rs`.
- This project targets macOS 26+ on Apple Silicon. Follow `README.md` for native prerequisites; do not add other platform support unless requested.
- Use npm and the existing `package-lock.json`. Check the manifests for current versions and scripts; do not assume commands or dependencies exist.
- Keep the pinned `swift-rs` versions in `src-tauri/Cargo.toml` and `AppleBridge/Package.swift` aligned when changing the bridge.
- Keep Apple framework logic in `AppleBridgeCore` and conversion/forwarding code in `AppleBridgeFFI`. Preserve the on-device approach unless the developer requests a change.

## Everyday coding habits

- Read the affected code and nearby conventions before editing. Fix the cause of a problem rather than adding another workaround.
- Use descriptive names, small cohesive functions, and guard clauses where they make control flow clearer. Avoid clever one-liners and arbitrary file-size rules.
- Keep related code together. Extract a component, hook, or module when it has a clear responsibility or useful reuse; do not create a file for every helper.
- Keep calculations pure where practical and isolate side effects at clear boundaries.
- Let TypeScript infer obvious local types. Explicitly type component props and cross-language contracts. Prefer unions that represent valid states over collections of contradictory booleans.
- Avoid `any`, unchecked casts, non-null assertions, and warning suppressions. Narrow unknown values and fix the underlying type issue; explain unavoidable exceptions locally.
- Comments should explain intent, constraints, or a non-obvious tradeoff. Remove dead code and obsolete comments within the changed scope.
- Do not hand-edit generated schemas, build outputs, or dependency lockfiles; use the owning tool when regeneration is necessary.
- Use existing dependencies first. Add a dependency only when it materially simplifies required behavior; avoid packages for trivial helpers.

## React patterns

- Use function components and follow the Rules of Hooks. Keep rendering pure and update state immutably.
- Keep state near its consumers. Lift it only as far as necessary; introduce context or a state library only for a demonstrated need.
- Derive values from props and state during rendering. Do not store duplicate or derived state, or mirror props into state without a deliberate reason.
- Put user-triggered work in event handlers. Use Effects to synchronize with external systems, not to chain state updates or calculate display values.
- Clean up listeners, timers, and subscriptions. Handle stale asynchronous results and unmounts; cleanup must also work under Strict Mode's extra development setup/cleanup cycle.
- Keep Effect dependencies accurate. Do not suppress dependency warnings to force an Effect to run once.
- Add memoization only for a measured performance issue or a concrete identity requirement.
- Use semantic HTML, labels, keyboard-accessible controls, visible focus, and stable list keys. Show the loading, empty, and error states needed by the interaction.

## Tauri and native patterns

- Keep presentation and interaction state in React. Keep privileged OS operations and authoritative native validation in Rust or the existing Swift bridge.
- Use Tauri 2 commands for request/response work. Keep command names, argument casing, and serialized payloads aligned with TypeScript; `invoke<T>` is a type annotation, not runtime validation.
- Keep command handlers focused. Extract ordinary Rust functions when business logic benefits from independent testing; avoid building a generic IPC framework.
- Use events for notifications and channels when ordered streaming is required. Always release event listeners, including when asynchronous registration finishes after a component unmounts.
- Keep slow work off the UI thread. Use async APIs for asynchronous I/O and `tauri::async_runtime::spawn_blocking` for blocking work that is safe to run there. Respect native APIs that require the main thread; adding `async` alone does not make blocking code nonblocking.
- Return `Result` for recoverable failures and propagate with `?`. Handle rejected commands in the UI. Do not use `unwrap`/`expect` for expected input, permission, I/O, or network failures, or silently turn errors into success.
- Keep shared mutable state limited and lock scopes short. Do not hold a synchronous mutex guard across `.await`.
- Treat frontend arguments as untrusted. Validate them at the native boundary and grant only the capabilities and scopes the feature needs. Custom commands need their own validation; capabilities do not make arbitrary Rust code safe.
- Do not broaden permissions, disable security controls, or introduce remote content to work around an implementation problem. Keep secrets and private user content out of frontend bundles and logs.
- Keep FFI and `unsafe` code contained in the bridge; document ownership, lifetime, and thread-safety assumptions when modifying it.

## Verification and handoff

Run checks appropriate to the changed behavior. Commands below are existing scripts or standard Cargo checks, not a requirement to run every command for every edit.

| Change | Check |
| --- | --- |
| Frontend code | `npm run build` (TypeScript check and Vite build) |
| Rust code | From `src-tauri/`: `cargo fmt --check`, `cargo check`, `cargo clippy --all-targets -- -D warnings` |
| Rust behavior/tests | From `src-tauri/`: `cargo test` |
| Swift bridge | From `AppleBridge/`: `swift build`; `swift run bridge-test` for the existing smoke CLI |
| Native integration or UI behavior | `npm run tauri dev`; exercise the affected flow in the actual app |
| Packaging or release configuration | `npm run tauri build` |

Run Cargo commands **inside `src-tauri/`** so its `.cargo/config.toml` is loaded. Native checks need the macOS/Swift toolchain described in `README.md`. Browser-only `npm run dev` does not verify native commands or permissions.

- There are currently no frontend lint or test scripts. Do not invent successful checks or add a test framework for a documentation or cosmetic change.
- Add focused tests for meaningful logic, regressions, and failure paths when warranted. Test observable behavior rather than implementation details; avoid redundant snapshots and mocks of everything.
- Review the final diff for unnecessary code, unintended changes, and unhandled failure paths. Documentation-only changes need a documentation review, not an application build.
- Report what changed, which checks ran, and any checks blocked by the environment. Clearly separate remaining required work from optional TODOs.

## Sources

These project rules adapt the following primary guidance; they do not require adopting every pattern or dependency shown in the examples.

- [Google engineering practices: review for complexity and over-engineering](https://github.com/google/eng-practices/blob/master/review/reviewer/looking-for.md).
- [React: choosing the state structure](https://react.dev/learn/choosing-the-state-structure), [avoiding unnecessary Effects](https://react.dev/learn/you-might-not-need-an-effect), and [Rules of Hooks](https://react.dev/reference/rules/rules-of-hooks).
- [Tauri: calling Rust](https://v2.tauri.app/develop/calling-rust/), [events and channels](https://v2.tauri.app/develop/calling-frontend/), [capabilities](https://v2.tauri.app/security/capabilities/), and [blocking tasks](https://docs.rs/tauri/latest/tauri/async_runtime/fn.spawn_blocking.html).
- [Rust: recoverable errors with Result](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html).
