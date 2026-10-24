# Testing

`cargo test` — 35 tests plus 1 doc test, about a second.

| Suite | File | What it proves |
|---|---|---|
| ISA | `tests/isa.rs` | Every arithmetic/logic op against known values; wrapping, signed division, logical shift; all branch forms; word and byte memory ops; recursive calls with a balanced stack; catchable and uncatchable traps for every trap code; heap alignment and OOM; `write`/`ticks`/`rand`; running off the end halts. |
| Sandbox | `tests/sandbox.rs` | Gas stops an infinite loop and resume continues *exactly* N iterations; gas totals are asserted to the unit; each capability denies then permits; host functions need both registration and capability, and host errors become traps; stack and call-depth caps; code is not addressable memory; pointer/length checks on syscalls. |
| Determinism | `tests/determinism.rs` | Two runs ⇒ equal `state_hash` and events, different seed ⇒ different; a mid-run snapshot restored into a fresh VM resumes to the same end state as an uninterrupted run; program and snapshot serialisation round-trip; traces of `run` and single-`step` execution are identical; corrupt files are rejected. |
| Assembler | `tests/asm.rs` | Disassembler output re-assembles to identical code; data directives, labels, character and hex immediates; every error kind carries a line number; comments inside quotes; the binary encoding is byte-exact. |
| Allocation bounds | `tests/alloc_bounds.rs` | A counting global allocator (test-only) proves a corrupt snapshot event count or program header cannot make the host request more memory than the input justifies. |
| CLI | `tests/cli.rs` | `--trace` prints the trace; the debugger's memory dump survives huge addresses; exit codes 0/3 as documented. |
| Doc test | `src/lib.rs` | The README's 4-line embedding example. |

## Real execution in CI

The workflow assembles and runs `examples/fib.hasm` and checks the printed
`75025` and the exit code, runs `guarded.hasm` and checks exit code 3
(out of gas) with both "caught trap" lines present, runs `checksum.hasm`
over a known input and checks the hex digest, and runs the `embed` example.

## Benchmarks

`cargo run --release --example bench`. Not asserted in CI; compare by eye
when touching `Vm::step`.

## Writing a test

Assemble a tiny program with `asm::assemble`, run it with `Vm::new(&p,
Config { .. })`, assert on `status`, `regs`, `mem`, `output`, `events`,
`gas_used` or `state_hash()`. Prefer programs to hand-built `Instr` values
except when testing the encoder.
