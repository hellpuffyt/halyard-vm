//! The security properties: gas bounds every program, capabilities gate
//! every host interaction, and memory/stack limits hold.

use halyard::{Cap, Config, Status, Vm, asm, trap};

fn cfg(caps: Vec<Cap>) -> Config {
    Config {
        caps,
        ..Default::default()
    }
}

#[test]
fn infinite_loop_is_stopped_by_gas_and_can_resume() {
    let p = asm::assemble("loop:\n addi r1, r1, 1\n jmp loop").unwrap();
    let mut vm = Vm::new(&p, Config::default());
    assert_eq!(vm.run(1_000), Status::OutOfGas);
    assert_eq!(vm.gas_used, 1_000);
    let r1 = vm.regs[1];
    assert_eq!(vm.resume(500), Status::OutOfGas);
    assert_eq!(vm.gas_used, 1_500);
    assert_eq!(
        vm.regs[1],
        r1 + 250,
        "exactly 250 more iterations of 2 gas each"
    );
}

#[test]
fn gas_accounting_is_exact() {
    // movi(1) + ld(2) + mul(3) + alloc(5) + sys ticks(10) + halt(1) = 22
    let p = asm::assemble(
        "movi r1, 8\n ld r2, r1, 0\n mul r3, r1, r1\n alloc r4, r1\n sys ticks\n halt",
    )
    .unwrap();
    let mut vm = Vm::new(&p, Config::default());
    assert_eq!(vm.run(100), Status::Halted);
    assert_eq!(vm.gas_used, 22);
    assert_eq!(
        vm.regs[0], 21,
        "ticks reports gas charged before the syscall completes"
    );
}

#[test]
fn capabilities_gate_syscalls() {
    let src = ".data m \"x\"\n movi r1, m\n movi r2, 1\n sys write\n halt";
    let p = asm::assemble(src).unwrap();
    let mut vm = Vm::new(&p, cfg(vec![]));
    assert_eq!(vm.run(100), Status::Trapped(trap::CAP_DENIED));
    assert!(vm.output.is_empty());
    let mut vm = Vm::new(&p, cfg(vec![Cap::Output]));
    assert_eq!(vm.run(100), Status::Halted);
    assert_eq!(vm.output, b"x");

    let p = asm::assemble("movi r1, 0\n movi r2, 1\n sys emit\n halt").unwrap();
    assert_eq!(
        Vm::new(&p, cfg(vec![Cap::Output])).run(100),
        Status::Trapped(trap::CAP_DENIED)
    );
    let mut vm = Vm::new(&p, cfg(vec![Cap::Emit]));
    assert_eq!(vm.run(100), Status::Halted);
    assert_eq!(vm.events.len(), 1);

    let p = asm::assemble("movi r1, 0\n movi r2, 64\n sys read\n halt").unwrap();
    let mut c = cfg(vec![Cap::Input]);
    c.input = b"payload".to_vec();
    let mut vm = Vm::new(&p, c);
    assert_eq!(vm.run(100), Status::Halted);
    assert_eq!(vm.regs[0], 7);
    assert_eq!(&vm.mem[..7], b"payload");
    assert_eq!(
        Vm::new(&p, cfg(vec![])).run(100),
        Status::Trapped(trap::CAP_DENIED)
    );
}

#[test]
fn host_functions_need_registration_and_capability() {
    let p = asm::assemble("movi r0, 7\n movi r1, 20\n movi r2, 22\n sys host\n halt").unwrap();
    // registered but not permitted
    let mut vm = Vm::new(&p, cfg(vec![]));
    vm.register_host(7, Box::new(|_, a| Ok(a[0] + a[1])));
    assert_eq!(vm.run(100), Status::Trapped(trap::CAP_DENIED));
    // permitted but not registered
    let mut vm = Vm::new(&p, cfg(vec![Cap::Host(7)]));
    assert_eq!(vm.run(100), Status::Trapped(trap::BAD_SYSCALL));
    // both: result lands in r0, and the host may touch memory
    let mut vm = Vm::new(&p, cfg(vec![Cap::Host(7)]));
    vm.register_host(
        7,
        Box::new(|mem, a| {
            mem[0] = 0xAB;
            Ok(a[0] + a[1])
        }),
    );
    assert_eq!(vm.run(100), Status::Halted);
    assert_eq!(vm.regs[0], 42);
    assert_eq!(vm.mem[0], 0xAB);
    // host errors become a catchable trap, not a panic
    let mut vm = Vm::new(&p, cfg(vec![Cap::Host(7)]));
    vm.register_host(7, Box::new(|_, _| Err("boom".into())));
    assert_eq!(vm.run(100), Status::Trapped(trap::HOST_ERROR));
}

#[test]
fn stack_and_call_depth_are_bounded() {
    let p = asm::assemble("loop:\n push r1\n jmp loop").unwrap();
    let mut vm = Vm::new(
        &p,
        Config {
            max_stack: 100,
            ..Default::default()
        },
    );
    assert_eq!(vm.run(10_000), Status::Trapped(trap::STACK_OVERFLOW));
    assert_eq!(vm.stack.len(), 100);
    let p = asm::assemble("f:\n call f").unwrap();
    let mut vm = Vm::new(
        &p,
        Config {
            max_call_depth: 50,
            ..Default::default()
        },
    );
    assert_eq!(vm.run(10_000), Status::Trapped(trap::STACK_OVERFLOW));
    assert_eq!(vm.calls.len(), 50);
}

#[test]
fn code_is_not_addressable_memory() {
    // Writing over "code" only writes data memory; the program keeps running its original code.
    let p = asm::assemble("movi r1, 0\n movi r2, 0x0101\n st r2, r1, 0\n st r2, r1, 8\n addi r3, r3, 1\n movi r4, 3\n jlt r3, r4, 2\n halt").unwrap();
    let mut vm = Vm::new(&p, Config::default());
    assert_eq!(vm.run(1000), Status::Halted);
    assert_eq!(vm.regs[3], 3);
    assert_eq!(vm.code()[2].op, halyard::Op::St as u8);
}

#[test]
fn write_length_and_pointer_are_checked() {
    let p = asm::assemble("movi r1, 65530\n movi r2, 100\n sys write\n halt").unwrap();
    assert_eq!(
        Vm::new(&p, Config::default()).run(100),
        Status::Trapped(trap::MEM_OOB)
    );
    let p = asm::assemble("movi r1, 0\n movi r2, -1\n sys write\n halt").unwrap();
    assert_eq!(
        Vm::new(&p, Config::default()).run(100),
        Status::Trapped(trap::MEM_OOB)
    );
}
