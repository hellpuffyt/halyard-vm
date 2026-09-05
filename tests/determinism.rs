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
