# User Testing

## Validation Surface

Use the workspace root `/home/linxy/repo/ciri` for all commands.

Primary automated validation commands:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

Safe executable smoke test:

```bash
cargo run -p ciri -- --help
```

Known limitation:
- Do **not** use `cargo run -p ciri-server -- --help` for this mission. The server help path is considered unsafe for this validation pass.

Manual / GUI validation:
- Only use GUI/manual checks if automated validation does not sufficiently cover the suspected bug-review fix.
- If a manual check is needed, prefer a minimal launch path and stop once the targeted behavior is confirmed.
- Headless GPU test surface is available, so prefer headless/render-safe validation over interactive desktop testing when possible.

Validation intent for this mission:
- Confirm Rust workspace bug-review fixes compile cleanly.
- Confirm tests pass without regressions.
- Confirm clippy is warning-free because CI treats warnings as errors.
- Confirm formatting is unchanged and compliant.
- Use the `ciri --help` path only as a low-risk executable smoke check.

## Validation Concurrency

Resource guidance for this machine:
- 24 CPUs available.
- Approximately 18 GB RAM available.

Execution guidance:
- Prefer **one heavy cargo validation job at a time**.
- Treat `cargo build`, `cargo test`, and `cargo clippy` as heavy jobs; do not overlap them.
- `cargo fmt --all -- --check` is lighter, but still run it separately to keep logs easy to attribute.
- Avoid running multiple workspace-wide cargo commands concurrently, since parallel Rust compilation plus tests can create unnecessary memory pressure and noisy failures.

Recommended order:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
cargo run -p ciri -- --help
```

If narrowing scope is necessary during debugging:
- Reduce to the smallest affected crate or test target first.
- After a scoped repro/fix check, restore full workspace validation before declaring success.

## Mission Notes

Mission focus:
- This testing draft supports validation of Rust workspace bug-review fixes.
- Prefer reproducible command-line validation over exploratory manual testing.
- Keep validator output attributable to a single command at a time.

Operational notes:
- Run commands from `/home/linxy/repo/ciri`.
- Expect inline Rust tests inside crate source files rather than separate `tests/` directories.
- CI behavior is strict on warnings, so any warning should be treated as a validation failure.
- Do not expand coverage beyond the requested validation surface unless the target fix specifically requires it.

## Flow Validator Guidance: cargo

Isolation rules:
- Stay inside `/home/linxy/repo/ciri` and only use read-only inspection plus cargo commands unless explicitly directed to write a flow report.
- Do not start long-lived servers or bind ports.
- Do not invoke `cargo run -p ciri-server -- --help`.
- Treat cargo builds/tests/clippy as shared heavy resources; only one validator should run a heavy cargo command at a time for this milestone.
- Do not modify source files, git state, or mission files other than the assigned JSON flow report and evidence output.

Safe boundaries:
- Temporary cargo artifacts under the existing workspace `target/` directory are allowed.
- Evidence should be textual command output captured in the flow report; do not create extra large artifacts unless the command specifically emits them.
