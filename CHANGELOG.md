# Changelog

Format: [Keep a Changelog](https://keepachangelog.com). Versions: SemVer.
Program-file and snapshot format changes are called out explicitly.

## [Unreleased]

### Fixed
- `Vm::restore` reserved memory from an untrusted 32-bit event count
  (~100 GB request from a 4-byte edit); it now never reserves more than the
  snapshot's remaining bytes justify.
- `Vm::restore` accepted ragged stack blobs, stacks deeper than the VM's
  limits, handlers outside the code, unknown status bytes and trailing
  bytes; all are now rejected.
- The assembler aborted the process on huge `.data` reservations; the data
  segment is now capped at 16 MiB with a line-numbered error.
- `halyard run --trace` did not print the trace unless an undocumented
  environment variable was set.
- Debugger `m A N` overflowed (panic in debug builds) for huge addresses.
- CI "No unsafe / no nondeterminism" step could never fail (`! grep` is
  exempt from `bash -e`); `#![forbid(unsafe_code)]` now enforces the first.

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
