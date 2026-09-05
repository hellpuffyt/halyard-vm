# Changelog

Format: [Keep a Changelog](https://keepachangelog.com). Versions: SemVer.
Program-file and snapshot format changes are called out explicitly.

## [Unreleased]

## [0.1.0] — 2026-09-06

First release. Program format v1, snapshot format `HSNP`.

### Added
- 38-instruction register ISA with fixed 8-byte encoding, wrapping i64
  arithmetic, word/byte memory ops, data and call stacks, bump heap.
- Gas metering with per-instruction costs; `run`/`resume` preemption.
- Trap model with one-shot in-program handler (`seth`) and ten trap codes.
- Syscalls `write`, `ticks`, `rand`, `host`, `emit`, `read` behind
  `Cap::{Output, Emit, Host(id), Input}`.
- Deterministic execution, `state_hash`, execution trace, snapshot/restore.
- Assembler (labels, `.data`, `.word`, char/hex literals, line-numbered
  errors), disassembler, CLI (`asm`, `run`, `dis`, `debug`).
- Examples: hello, recursive fib with itoa, FNV-1a checksum over host input,
  guarded misbehaving program, embedding with a host function, benchmark.
