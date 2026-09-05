//! `cargo run --release --example bench` — interpreter throughput.

use std::time::Instant;

use halyard::{Config, Status, Vm, asm};

fn bench(name: &str, src: &str, gas: u64) {
    let p = asm::assemble(src).unwrap();
    let mut vm = Vm::new(&p, Config::default());
    let t = Instant::now();
    let st = vm.run(gas);
    let secs = t.elapsed().as_secs_f64();
    let n = vm.trace.len().max(1) as f64;
    let _ = n;
    println!(
        "{name:<34} {st:<12} {:>9.0} k instr/s   {:>7.1} ns/instr   gas {}",
        vm.gas_used as f64 / secs / 1e3 * (instr_per_gas(src)),
        secs * 1e9 / (vm.gas_used as f64 * instr_per_gas(src)),
        vm.gas_used
    );
}

// Rough instructions-per-gas for each workload so the number printed is instructions.
fn instr_per_gas(src: &str) -> f64 {
    if src.contains("ld ") || src.contains("push") {
        0.75
    } else {
        1.0
    }
}

fn main() {
    bench(
        "tight loop (addi + jlt)",
        "movi r2, 100000000\n loop:\n addi r1, r1, 1\n jlt r1, r2, loop\n halt",
        50_000_000,
    );
    bench(
        "memory loop (ld/st/addi/jlt)",
        ".data buf 64\n movi r2, 100000000\n movi r3, buf\n loop:\n ld r4, r3, 0\n add r4, r4, r1\n st r4, r3, 0\n addi r1, r1, 1\n jlt r1, r2, loop\n halt",
        50_000_000,
    );
    bench(
        "calls (push/call/pop/ret)",
        "movi r2, 100000000\n loop:\n push r1\n call f\n pop r1\n addi r1, r1, 1\n jlt r1, r2, loop\n halt\n f:\n ret",
        50_000_000,
    );
    let p = asm::assemble("movi r1, 24\n call fib\n halt\n fib:\n movi r2, 2\n jlt r1, r2, base\n push r1\n addi r1, r1, -1\n call fib\n pop r1\n push r0\n addi r1, r1, -2\n call fib\n pop r2\n add r0, r0, r2\n ret\n base:\n mov r0, r1\n ret").unwrap();
    let mut vm = Vm::new(&p, Config::default());
    let t = Instant::now();
    assert_eq!(vm.run(u64::MAX / 2), Status::Halted);
    let secs = t.elapsed().as_secs_f64();
    println!(
        "fib(24) recursive = {}                  {:.1} ms, gas {}",
        vm.regs[0],
        secs * 1e3,
        vm.gas_used
    );

    let t = Instant::now();
    let snap = vm.snapshot();
    let mut vm2 = Vm::new(&p, Config::default());
    vm2.restore(&snap).unwrap();
    println!(
        "snapshot + restore (64 KiB)              {:.1} µs, {} bytes",
        t.elapsed().as_secs_f64() * 1e6,
        snap.len()
    );
    let t = Instant::now();
    let h = vm2.state_hash();
    println!(
        "state hash (64 KiB)                      {:.1} µs -> {h:016x}",
        t.elapsed().as_secs_f64() * 1e6
    );
}
