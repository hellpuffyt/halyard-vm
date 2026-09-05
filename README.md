# Halyard

**A deterministic, gas-metered, capability-gated virtual machine for running
code you don't trust — and replaying exactly what it did.**

Halyard is a small register VM (16 × i64, fixed 8-byte instructions) built for
one job: executing automation steps — agent tools, workflow actions, plugins —
where the host must bound *how long* the code runs, control *what* it can
touch, and be able to *prove* what happened. Zero dependencies, pure Rust,
~1,500 lines including the assembler and debugger.

```
$ halyard run examples/fib.hasm --hash
75025
[halted; gas used 2913492; r0 = 6]
state hash: b139eb51d3d172a3        # identical on every machine, every time

$ halyard run examples/guarded.hasm --gas 5000
caught trap 2                       # out-of-bounds read, caught in-program
caught trap 7                       # syscall without capability, caught
[out of gas; gas used 5000; r0 = 1] # infinite loop, stopped by the meter
$ echo $?
3
```

## What is it?

| Property | How |
|---|---|
| **Deterministic** | No wall clock, no OS randomness, no threads. `sys ticks` is the gas counter; `sys rand` is a seeded xorshift. `Vm::state_hash()` fingerprints the whole machine; equal inputs ⇒ equal hashes. |
| **Metered** | Every instruction has a fixed gas cost. `run(gas)` stops with `OutOfGas` and can be `resume`d — preemption without threads. |
| **Capability-gated** | The only ways out of the sandbox are numbered syscalls, each behind an explicit `Cap` (`Output`, `Input`, `Emit`, `Host(id)`). Missing capability ⇒ trap, not silent no-op. |
| **Resumable** | `snapshot()` / `restore()` serialise registers, memory, stacks, heap, handler, gas and RNG. Pause a workflow step, persist it, resume it on another machine, and end in the same state. |
| **Trap model** | Division by zero, out-of-bounds memory, stack overflow, bad jumps, illegal instructions, denied capabilities and user `trap n` all deliver a code to an in-program handler (`seth`) or halt the VM with `Trapped(code)`. |
| **Harvard layout** | Code lives outside data memory. No self-modifying code, no jumping into data. |

Tooling: an assembler with labels, data directives and character literals; a
disassembler; an interactive debugger (`step`, `continue`, breakpoints,
registers, memory dump, stacks, state hash); an execution trace.

## Who is it for?

- **Agent and workflow platforms** that let models or users supply small
  programs and need them bounded, auditable and replayable.
- **Plugin hosts** that want a sandbox smaller than WebAssembly and fully
  under their control.
- **Anyone learning VM design** who wants a complete, readable
  implementation: ISA, encoder, interpreter, traps, syscalls, snapshotting,
  tooling.

## Why does it exist?

WebAssembly is the right answer for portable, fast sandboxing — and it is also
a large specification with JIT engines you cannot read in an afternoon, no
built-in gas metering, and no notion of a serialisable machine state. Lua and
similar embed nicely but are not deterministic by default and cannot be
paused and resumed byte-for-byte. Halyard trades speed and expressiveness for
the properties an orchestrator actually needs: **bounded**, **deterministic**,
**inspectable**, **resumable**. It is the execution substrate for
"run this untrusted step, stop it after N units of work, tell me exactly what
it did, and pick it up later if needed."

## What makes it different?

- **Gas is part of the ISA, not a wrapper.** Costs are documented per
  instruction; `sys ticks` exposes the meter to the program itself.
- **Capabilities are per-syscall and per-host-function id.** A program with
  `Host(7)` cannot call host function 8. The host registers closures; the VM
  passes them a mutable view of guest memory and three arguments.
- **Snapshots are the unit of scheduling.** A `Vm` that ran out of gas is a
  complete, serialisable value.
- **Traps are recoverable in-program** with a one-shot handler, so a step can
  clean up and report rather than just die.
- **Fixed-width, trivially decodable instructions** make the disassembler
  exact and the trace cheap.

## Why is this not just a tutorial?

Because the hard parts are done and tested: every trap has a test; gas
accounting is asserted to the unit; capabilities are tested from both sides
(denied and granted, registered and unregistered); snapshots are proven to
resume to the same `state_hash` as an uninterrupted run; corrupt program and
snapshot files are rejected without panicking; and there is a real assembler
with line-numbered errors and a real debugger, not a `println!` loop.

## Benchmarks

`cargo run --release --example bench`, Windows 11 laptop, one run:

| Workload | Throughput |
|---|---|
| Tight loop (`addi` + `jlt`) | ≈ 660 M instr/s (1.5 ns/instr) |
| Memory loop (`ld`/`add`/`st`/`addi`/`jlt`) | ≈ 540 M instr/s |
| Call-heavy (`push`/`call`/`pop`/`ret`) | ≈ 630 M instr/s |
| `fib(24)` recursive | 2.5 ms, 1.8 M gas |
| Snapshot + restore, 64 KiB memory | 67 µs, 65,729 bytes |
| State hash, 64 KiB | 46 µs |

It is a switch-dispatch interpreter with bounds checks on every memory access;
that is the ceiling. It is fast enough that gas, not CPU, is the limit you
will hit.

## Build, test, run

```
cargo build --release              # target/release/halyard
cargo test                         # 28 tests: ISA, sandbox, determinism, assembler
cargo run --release --example bench
cargo run --example embed          # host function + capability demo

halyard asm  examples/fib.hasm -o fib.hy
halyard dis  fib.hy
halyard run  fib.hy --gas 5000000 --hash
halyard run  examples/checksum.hasm --input README.md --cap input,emit,output
halyard debug examples/fib.hasm    # s, c, r, m, b, d, st, hash, q
```

Exit codes: 0 halted, 3 out of gas, 4 trapped.

## Embedding

```rust
use halyard::{Cap, Config, Vm, asm};

let prog = asm::assemble("movi r0, 1\n movi r1, 41\n sys host\n halt")?;
let mut vm = Vm::new(&prog, Config { caps: vec![Cap::Host(1)], ..Default::default() });
vm.register_host(1, Box::new(|_mem, args| Ok(args[0] + 1)));
vm.run(1_000);
assert_eq!(vm.regs[0], 42);
```

## Documentation

- [`docs/ISA.md`](docs/ISA.md) — every instruction, its encoding and gas cost; syscalls; traps; assembler syntax
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — machine model, execution loop, snapshot format
- [`SECURITY.md`](SECURITY.md) — what the sandbox guarantees and what it doesn't
- [`TESTING.md`](TESTING.md), [`ROADMAP.md`](ROADMAP.md), [`CONTRIBUTING.md`](CONTRIBUTING.md), [`CHANGELOG.md`](CHANGELOG.md)

## Why star or contribute?

Star it if you want a sandbox whose entire trusted computing base fits in one
file you can audit. Contribute if you want to add a compiler front end (the
ISA is a comfortable target), a register allocator, a JIT, or a new
capability — each is a self-contained project against a stable, tested core.

## License

MIT. See [`LICENSE`](LICENSE).
