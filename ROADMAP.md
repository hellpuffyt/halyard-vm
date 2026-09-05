# Roadmap

Each item is one reviewable PR.

## 0.2 — richer programs

- [ ] `free` / free-list inside the arena for long-running steps.
- [ ] Indirect calls (`calli ra`) so compiled code can use function pointers.
- [ ] 32-bit and 16-bit load/store (`ldw`, `stw`, `ldh`, `sth`).
- [ ] `halyard trace` command: print the executed pc sequence with instructions.
- [ ] Assembler macros for common idioms (`.string`, `.align`).

## 0.3 — a compiler target

- [ ] A tiny expression language → Halyard compiler in `examples/`, proving the
  ISA is a comfortable target.
- [ ] Symbol table in the program file (optional section) so the debugger
  shows labels.
- [ ] `halyard bench` with a fixed corpus and JSON output for regression tracking.

## 0.4 — orchestration features

- [ ] Multiple VMs scheduled round-robin by gas slice in one host thread.
- [ ] Memory-bounded output/events (`Config::max_output`).
- [ ] Signed snapshots (host-supplied MAC hook; never a home-grown cipher).

## Someday

- A baseline JIT for x86-64 with identical semantics and gas accounting.
- A WebAssembly build of the interpreter for running Halyard in browsers.

## Non-goals

- Floating point. Determinism across platforms is easier without it; use
  fixed-point in-program.
- Becoming a general-purpose language runtime.
