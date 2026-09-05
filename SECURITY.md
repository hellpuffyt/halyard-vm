# Security

Halyard is a sandbox. This document says exactly what that means.

## Guarantees (tested)

| Guarantee | Mechanism | Test |
|---|---|---|
| A program cannot run longer than the host allows | Gas charged on every instruction; `run(gas)` returns `OutOfGas` | `sandbox::infinite_loop_is_stopped_by_gas_and_can_resume`, `gas_accounting_is_exact` |
| A program cannot read or write outside its arena | Every access through checked `addr()`; negative, overflowing and straddling ranges trap | `isa::uncaught_traps_stop_the_machine`, `sandbox::write_length_and_pointer_are_checked` |
| A program cannot modify or jump into its own code | Harvard layout: code is not in memory; jumps are range-checked | `sandbox::code_is_not_addressable_memory`, `BAD_JUMP` test |
| A program cannot exhaust host memory | Fixed arena; `alloc` traps with `OOM`; stacks have hard caps | `isa::heap_allocation`, `sandbox::stack_and_call_depth_are_bounded` |
| A program cannot reach the host without permission | Each boundary syscall checks a `Cap`; host functions need `Cap::Host(id)` *and* registration | `sandbox::capabilities_gate_syscalls`, `host_functions_need_registration_and_capability` |
| A host error cannot crash the VM | `Err` from a host closure becomes `HOST_ERROR` trap | same |
| Malformed programs and snapshots cannot crash the loader | Length/overflow-checked decoding | `determinism::corrupt_program_files_are_rejected`, `snapshot_survives_…` |
| Behaviour is reproducible | No time, no OS randomness, no threads; `state_hash` | `determinism::*` |
| No memory-unsafety in the VM itself | 100% safe Rust, zero `unsafe`, zero dependencies | CI grep |

## Non-guarantees

- **Host functions are trusted.** A closure you register gets `&mut [u8]`
  over guest memory and can do anything your process can. The capability
  system controls *whether the guest may call it*, not what it does.
- **Side channels.** Gas usage and `state_hash` are observable; timing is
  not constant. Do not rely on Halyard to hide secrets from the guest program
  that you have placed in its memory.
- **Denial of service by allocation.** `Config::memory_size` is allocated
  up front by the host. Choose it deliberately; the guest cannot grow it.
- **Output size.** `write`/`emit` append to host-side vectors bounded only
  by gas × 10 bytes per syscall at most 64 KiB each. If you run with a very
  large gas budget, cap `output.len()` yourself between `run` calls or lower
  the budget.
- **Snapshots are not authenticated.** A tampered snapshot restores to a
  tampered state. Sign them if they cross a trust boundary.

## Reporting

Open a security advisory on the GitHub repository or email the maintainer in
`Cargo.toml`. Include a `.hasm` reproducer where possible. Acknowledgement
within 7 days; only the latest 0.x release receives fixes.
