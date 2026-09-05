# Architecture

```
 .hasm text ──► asm::assemble ──► Program { code: Vec<Instr>, data: Vec<u8> }
                                       │  to_bytes / from_bytes  ("HLYD" v1)
                                       ▼
                               Vm::new(&program, Config)
                                       │
        ┌──────────────────────────────┴──────────────────────────────┐
        │ Vm                                                          │
        │  regs[16]: i64   pc: u32   gas_used: u64   status           │
        │  mem: Vec<u8>  (data segment at 0, bump heap above it)      │
        │  stack: Vec<i64> (≤ max_stack)   calls: Vec<u32> (≤ depth)  │
        │  handler: Option<u32>   rng: u64   caps   hosts   trace     │
        │  output: Vec<u8>   events: Vec<Vec<u8>>                     │
        └──────────────────────────────┬──────────────────────────────┘
                                       │ step() / run(gas) / resume(gas)
                                       ▼
                    Status::{Running, Halted, OutOfGas, Trapped(code)}
```

## Machine model

- **Registers** `r0`–`r15`, signed 64-bit, wrapping arithmetic. By convention
  `r0` is the return value / syscall result, `r1`–`r3` are syscall arguments.
- **Code** is a `Vec<Instr>` addressed by instruction index (`pc`). It is not
  in `mem`, so programs cannot read, write or jump into bytes they control.
  Running off the end is a clean halt.
- **Memory** is one flat arena (`Config::memory_size`, default 64 KiB). The
  program's data segment is copied to address 0; `alloc` bumps a heap pointer
  above it, 8-byte aligned. There is no free; a step's arena is discarded
  when the step ends. (ponytail: bump allocation, upgrade path is a free-list
  inside the arena if long-running programs need it.)
- **Data stack** and **call stack** are separate host-side vectors with hard
  caps, so `push`/`call` overflow is a trap, never a memory corruption, and a
  return address can never be overwritten by data.
- **Traps** carry an `i64` code. `raise` delivers it to the handler if one is
  armed (`r0 = code`, `pc = handler`, handler disarmed) or sets
  `Status::Trapped`. The handler is one-shot so a faulting handler cannot loop.

## Execution loop

`step()` decodes the instruction at `pc`, charges gas (see `docs/ISA.md`),
advances `pc`, executes, and returns the status. `run(gas)` loops until the
status changes or `gas_used` reaches the budget. `resume(gas)` flips
`OutOfGas` back to `Running` and calls `run`. No state is lost between the
two, which is what makes gas-based preemption work.

All register indices are masked to 4 bits *and* validated (`> 15` is an
`ILLEGAL` trap), so a hand-crafted instruction cannot index out of `regs`.
Every memory access goes through `addr(base, imm, len)`, which uses checked
arithmetic and rejects negative addresses and ranges that end past `mem`.

## Syscalls and capabilities

`sys n` dispatches on `n`. Each syscall that crosses the sandbox boundary
checks `caps` first:

| Syscall | Cap | Effect |
|---|---|---|
| `write` | `Output` | append `mem[r1..r1+r2]` to `output` |
| `emit` | `Emit` | push `mem[r1..r1+r2]` to `events` |
| `read` | `Input` | copy host input into `mem[r1..]`, at most `r2` bytes; `r0 = n` |
| `host` | `Host(r0)` | call the registered closure for id `r0` with `(r1, r2, r3)` and `&mut mem`; `r0 = result` |
| `ticks` | — | `r0 = gas_used` |
| `rand` | — | `r0 = next xorshift64\*` from the seed |

Host closures receive the *whole* guest memory. They are trusted code; the
capability decides whether the guest may invoke them, not what they may do.

## Determinism

The only sources of nondeterminism a VM could have — time, randomness,
scheduling, external I/O — are either absent or replaced: `ticks` is the gas
counter, `rand` is seeded, there are no threads, and I/O is only through
syscalls whose inputs are part of `Config`. `state_hash()` (FNV-1a over
registers, pc, memory, both stacks, heap pointer, handler, gas and RNG state)
is therefore a complete fingerprint: two VMs with equal hashes will behave
identically from here on.

## Snapshot format

`HSNP` magic, then registers, pc, heap, handler, gas, rng, status byte + trap
code, then length-prefixed blobs for memory, data stack, call stack, output,
and each event. `restore` validates lengths and that `pc`/`heap` fit the
current program and memory; it does not carry host closures (re-register
them) or the trace.

## Program file format

`HLYD`, `u32` version (1), `u32` instruction count, `u32` data length, then
instructions (8 bytes each: `op a b c imm:i32 LE`) and data bytes. The
loader checks the exact length with overflow-safe arithmetic.

## Tooling

- `asm.rs` — two-pass assembler (collect labels, then fix up immediates) and
  a disassembler whose output re-assembles to identical code.
- `main.rs` — `asm`, `run`, `dis`, `debug`. The debugger is ~100 lines over
  the public `Vm` API; nothing in it is privileged.
