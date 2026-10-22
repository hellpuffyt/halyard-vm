# Production readiness — halyard-vm

Audit date: 2026-10-22. Tested commit: `b8f7775` on branch
`audit/production-readiness` (this file was added in the next commit; no
code changed after the tested commit).

## Scope

Halyard is a deterministic, gas-metered, capability-gated register VM
(16 × i64, fixed 8-byte instructions, Harvard layout) with an assembler,
disassembler, interactive debugger, snapshots and a `state_hash`. Zero
dependencies, `#![forbid(unsafe_code)]`. Architecture:
[`../ARCHITECTURE.md`](../ARCHITECTURE.md); ISA: [`ISA.md`](ISA.md).
Maturity: experimental 0.1.0.

## Platforms and prerequisites

Pure Rust ≥ 1.88; no OS-specific code. Verified here on macOS 27 (Apple
silicon, rustc 1.98.0); default-branch CI runs ubuntu, windows and macos.

## Clean-clone verification (this audit)

```
git clone -b audit/production-readiness <repo> cc && cd cc
cargo build --release                                  # exit 0
cargo test                                             # exit 0: 36 passed, 0 failed, 0 ignored
cargo run --release --example bench                    # exit 0
cargo run --example embed                              # exit 0 ("plugin returned 6749")
halyard asm examples/fib.hasm -o fib.hy                # exit 0
halyard dis fib.hy                                     # exit 0
halyard run fib.hy --gas 5000000 --hash                # exit 0: 75025, state hash b139eb51d3d172a3
halyard run examples/checksum.hasm --input README.md --cap input,emit,output   # exit 0
printf 'q\n' | halyard debug examples/fib.hasm         # exit 0
```
(`halyard` = `target/release/halyard`.) The CI "real execution" script was
also replayed locally with `bash -eo pipefail`: all assertions pass.

## Commands executed (exit codes)

| Command | Before (main `e43efb7`) | After (`b8f7775`) |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 0 |
| `cargo clippy --all-targets -- -D warnings` | 0 | 0 |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 0 | 0 |
| `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --locked` | — | 0 |
| `cargo test --release` | 0: 29 passed (28 + 1 doc), 0 failed, 0 ignored | 0: 36 passed (35 + 1 doc), 0 failed, 0 ignored |
| `cargo test` (debug) | — | 0: 36 passed, 0 failed, 0 ignored |
| `cargo audit --db <advisory-db 2026-10-22> --no-fetch` | 0 | 0 (1 crate) |
| `gitleaks git` / `gitleaks dir` | — | no leaks found (3 + 6 commits) |

## CI status

Default branch (`main`, `e43efb7`): **success** (run 33985475947,
2026-09-05). Audit branch: pending until pushed. Note: the
"No unsafe / no nondeterminism" step on `main` could never fail (see fix 5).

## Bugs found and fixed

| # | Defect | Fix | Regression test |
|---|---|---|---|
| 1 | `Vm::restore` called `Vec::with_capacity(n_events)` with an untrusted `u32`: editing 4 bytes of a 65 KB snapshot made the host request 103,079,215,080 bytes (abort where overcommit refuses) — contradicts "malformed snapshots cannot crash the loader" | reserve at most `remaining_bytes / 4` | `alloc_bounds::corrupt_snapshot_event_count_cannot_force_a_huge_allocation` (counting allocator; failed before the fix with the 103 GB figure) |
| 2 | `restore` accepted ragged stack blobs (bytes silently dropped), stacks/call stacks beyond the VM's limits, handler outside code, unknown status byte, trailing bytes | validate before mutating | `determinism::inconsistent_snapshots_are_rejected` (failed before) |
| 3 | `.data buf 9223372036854775807` aborted the process ("memory allocation of 9223372036854775807 bytes failed") | data segment capped at 16 MiB (`asm::MAX_DATA_BYTES`), line-numbered error | `asm::oversized_data_segment_is_an_error_not_an_abort` |
| 4 | `halyard run --trace` never printed the trace unless an undocumented `HALYARD_TRACE` variable was set | print when `--trace` | `cli::trace_flag_prints_the_trace` |
| 5 | Debugger `m 18446744073709551615 16` panicked ("attempt to add with overflow") in debug builds | saturating add | `cli::debugger_memory_dump_handles_huge_addresses` |
| 6 | CI `! grep` checks for `unsafe` / `std::time` could never fail under `bash -e` | `#![forbid(unsafe_code)]` in both crate roots; checks rewritten as `if grep …; then exit 1; fi` | replayed locally |
| 7 | SECURITY.md pointed to a maintainer email in `Cargo.toml` that does not exist | link to private advisories | — |

## Security review

- Sandbox boundary: every memory access goes through checked `addr()`;
  jumps, handler, stacks and call depth are bounded; division uses
  wrapping ops (no `MIN / -1` panic); shifts are masked; syscalls check
  capabilities before touching host state. No panics found in `step()`.
- Untrusted inputs: program files (length/overflow-checked), snapshots (now
  fully validated, allocation-bounded), assembly source (now bounded).
- Documented non-guarantees kept: output/event growth bounded only by gas
  (64 KiB per 10-gas syscall), host functions are trusted, snapshots are not
  authenticated, data segment sets a floor on arena size (newly documented).
- History: no secret hits (pattern scan over all revisions + gitleaks).
  No rotation needed.
- Workflow: `permissions: contents: read`; actions pinned by tag (unchanged).

## Known limitations / unresolved risks

- Benchmark table in README is from a Windows laptop and was not reproduced;
  on this shared Mac the tight loop measured ≈ 277 M instr/s (README: ≈ 660 M).
- `Program::to_bytes` casts lengths to `u32`; hand-built `Program`s over
  4 GiB would serialise incorrectly (the assembler can no longer produce
  them).
- Output/event vectors can grow quickly under large gas budgets (documented).
- `gitleaks-action@v2` and `cargo install cargo-audit` CI steps were not run
  locally in their CI form; local equivalents pass.
- License is MIT only (kept).

## Acceptance checklist

| Item | Expected | Command / test | Result | Actual | Reference |
|---|---|---|---|---|---|
| Builds and tests from clean clone | exit 0 | `cargo build --release && cargo test` | pass | 36/36 | `tests/` |
| Determinism | equal inputs ⇒ equal `state_hash` | `determinism::identical_runs_produce_identical_state`; CLI fib hash | pass | `b139eb51d3d172a3` | `src/lib.rs` |
| Gas metering exact and resumable | stop at budget, resume exactly | `sandbox::infinite_loop_is_stopped_by_gas_and_can_resume`, `gas_accounting_is_exact` | pass | ok | `src/lib.rs` |
| Capabilities gate syscalls | denied ⇒ trap 7 | `sandbox::capabilities_gate_syscalls`, embed example | pass | ok | `src/lib.rs` |
| Traps catchable/uncatchable | handler or `Trapped` | `isa::*`, `guarded.hasm` exit 3 | pass | ok | `src/lib.rs` |
| Snapshot resume equivalence | same end hash | `determinism::snapshot_mid_run_resumes_to_the_same_end_state` | pass | ok | `src/lib.rs` |
| Corrupt programs/snapshots rejected without crash or huge alloc | `Err` | `determinism::*`, `alloc_bounds::*` | pass | ok | `src/lib.rs` |
| Assembler errors carry line numbers, no abort | `AsmError` | `asm::*` | pass | ok | `src/asm.rs` |
| CLI exit codes 0/3/4 and tools | as README | `cli::*`, clean-clone commands | pass | ok | `src/main.rs` |
| Dependency/secret audit | clean | `cargo audit`, gitleaks | pass | clean | — |

## Classification proposal

**Partially Verified** — all documented features verified on macOS from a
clean clone with new regression tests for every fix; Windows/Linux
coverage relies on CI (green on `main`, pending on this branch), and the
README benchmark figures could not be reproduced on this machine.
