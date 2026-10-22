//! Same program + same config ⇒ same state, every time; snapshots resume
//! to the identical end state; traces replay.

use halyard::{Cap, Config, Program, Status, Vm, asm};

const PROG: &str = "\
    .data buf 256\n\
    movi r1, 0\n movi r2, 200\n\
    loop:\n sys rand\n stb r0, r1, 0\n addi r1, r1, 1\n jlt r1, r2, loop\n\
    movi r1, 0\n movi r2, 16\n sys emit\n\
    movi r3, 1000\n alloc r4, r3\n sys ticks\n halt";

fn fresh(seed: u64, trace: bool) -> Vm {
    Vm::new(
        &asm::assemble(PROG).unwrap(),
        Config {
            seed,
            trace,
            caps: vec![Cap::Emit],
            ..Default::default()
        },
    )
}

#[test]
fn identical_runs_produce_identical_state() {
    let mut a = fresh(9, false);
    let mut b = fresh(9, false);
    assert_eq!(a.run(100_000), Status::Halted);
    assert_eq!(b.run(100_000), Status::Halted);
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(a.events, b.events);
    let mut c = fresh(10, false);
    c.run(100_000);
    assert_ne!(
        a.state_hash(),
        c.state_hash(),
        "seed changes the rand stream"
    );
}

#[test]
fn snapshot_mid_run_resumes_to_the_same_end_state() {
    let mut whole = fresh(3, false);
    whole.run(100_000);
    let mut part = fresh(3, false);
    assert_eq!(part.run(150), Status::OutOfGas);
    let snap = part.snapshot();
    let mut restored = fresh(3, false);
    restored.restore(&snap).unwrap();
    assert_eq!(restored.state_hash(), part.state_hash());
    assert_eq!(restored.resume(100_000), Status::Halted);
    assert_eq!(restored.state_hash(), whole.state_hash());
    assert_eq!(restored.output, whole.output);
    assert_eq!(restored.events, whole.events);
    assert_eq!(restored.gas_used, whole.gas_used);
}

#[test]
fn snapshot_survives_serialisation_round_trip_of_program_too() {
    let p = asm::assemble(PROG).unwrap();
    let p2 = Program::from_bytes(&p.to_bytes()).unwrap();
    assert_eq!(p, p2);
    let mut vm = Vm::new(&p2, Config::default());
    vm.run(50);
    let snap = vm.snapshot();
    let mut vm2 = Vm::new(&p2, Config::default());
    vm2.restore(&snap).unwrap();
    assert_eq!(vm.state_hash(), vm2.state_hash());
    assert!(vm2.restore(&snap[..20]).is_err());
    assert!(vm2.restore(b"garbage").is_err());
}

#[test]
fn trace_replays_exactly() {
    let mut a = fresh(1, true);
    let mut b = fresh(1, true);
    a.run(100_000);
    b.run(100_000);
    assert_eq!(a.trace, b.trace);
    assert_eq!(a.trace.len() as u64, a.trace.len() as u64);
    assert!(a.trace.len() > 600, "loop body executed 200 times");
    // stepping one instruction at a time gives the same trace
    let mut c = fresh(1, true);
    while c.step() == Status::Running {}
    assert_eq!(c.trace, a.trace);
    assert_eq!(c.state_hash(), a.state_hash());
}

#[test]
fn corrupt_program_files_are_rejected() {
    let p = asm::assemble("movi r1, 1\n halt").unwrap();
    let mut bytes = p.to_bytes();
    assert!(Program::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    bytes[0] = b'X';
    assert!(Program::from_bytes(&bytes).is_err());
    let mut huge = p.to_bytes();
    huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        Program::from_bytes(&huge).is_err(),
        "size overflow is an error, not a panic"
    );
}

/// Regression: `restore` accepted snapshots whose stacks were not a whole
/// number of entries (silently dropping bytes), exceeded the VM's stack
/// limits, named a handler outside the code, had unknown status bytes, or
/// carried trailing garbage.
#[test]
fn inconsistent_snapshots_are_rejected() {
    let p = asm::assemble(PROG).unwrap();
    let mut vm = Vm::new(&p, Config::default());
    vm.run(50);
    let good = vm.snapshot();
    let fresh_vm = || Vm::new(&p, Config::default());
    assert!(fresh_vm().restore(&good).is_ok());

    let mut trailing = good.clone();
    trailing.push(0);
    assert!(fresh_vm().restore(&trailing).is_err(), "trailing byte");

    // Layout: magic(4) regs(128) pc(4) heap(8) handler(4) gas(8) rng(8) status(1) code(8)
    let handler_at = 4 + 128 + 4 + 8;
    let status_at = handler_at + 4 + 8 + 8;
    let mut bad_handler = good.clone();
    bad_handler[handler_at..handler_at + 4].copy_from_slice(&1_000_000u32.to_le_bytes());
    assert!(
        fresh_vm().restore(&bad_handler).is_err(),
        "handler past code"
    );

    let mut bad_status = good.clone();
    bad_status[status_at] = 9;
    assert!(fresh_vm().restore(&bad_status).is_err(), "unknown status");

    // Rebuild a snapshot with a ragged stack blob (7 bytes).
    let mem_len_at = status_at + 1 + 8;
    let mem_len = u32::from_le_bytes(good[mem_len_at..mem_len_at + 4].try_into().unwrap()) as usize;
    let stack_at = mem_len_at + 4 + mem_len;
    let old_stack = u32::from_le_bytes(good[stack_at..stack_at + 4].try_into().unwrap()) as usize;
    let mut ragged = good[..stack_at].to_vec();
    ragged.extend_from_slice(&7u32.to_le_bytes());
    ragged.extend_from_slice(&[0; 7]);
    ragged.extend_from_slice(&good[stack_at + 4 + old_stack..]);
    assert!(fresh_vm().restore(&ragged).is_err(), "ragged stack");

    // A stack deeper than this VM allows.
    let mut deep = good[..stack_at].to_vec();
    let entries = Config::default().max_stack + 1;
    deep.extend_from_slice(&((entries * 8) as u32).to_le_bytes());
    deep.extend_from_slice(&vec![0; entries * 8]);
    deep.extend_from_slice(&good[stack_at + 4 + old_stack..]);
    assert!(fresh_vm().restore(&deep).is_err(), "stack over limit");
}
