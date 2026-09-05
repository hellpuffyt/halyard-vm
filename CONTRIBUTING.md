# Contributing

## Ground rules

- Zero dependencies, zero `unsafe`. CI enforces both.
- Every ISA change updates `docs/ISA.md` (encoding, gas cost, traps) and
  gets a test in `tests/isa.rs`.
- Every security-relevant change (memory, stacks, syscalls, capabilities,
  loaders) gets a test in `tests/sandbox.rs` or `tests/determinism.rs`.
- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` clean.
- Determinism is non-negotiable: no `std::time`, no `rand`, no threads in
  `src/lib.rs`.

## Workflow

```
git clone https://github.com/hellpuffyt/halyard-vm
cd halyard-vm
cargo test
cargo run --release --example bench     # before/after for changes to Vm::step
```

## Where things live

| Want to… | Look in |
|---|---|
| Add an instruction | the `ops!` table and `Vm::step` in `src/lib.rs`, gas cost in `step`, docs in `docs/ISA.md` |
| Add a syscall / capability | `sys` module, `Cap`, `Vm::syscall` |
| Change file formats | `Program::{to_bytes, from_bytes}`, `Vm::{snapshot, restore}` — bump the version |
| Assembler syntax | `src/asm.rs` |
| Debugger commands | `src/main.rs::debug` |

## Reporting bugs

A `.hasm` program plus the command line you ran is the ideal report.
