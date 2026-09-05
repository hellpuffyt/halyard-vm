//! Instruction semantics, driven through the assembler so the tests read
//! like programs.

use halyard::{Config, Status, Vm, asm, trap};

fn run(src: &str) -> Vm {
    let p = asm::assemble(src).expect("assemble");
    let mut vm = Vm::new(&p, Config::default());
    vm.run(1_000_000);
    vm
}

#[test]
fn arithmetic_and_logic() {
    let vm = run(
        "movi r1, 100\n movi r2, 7\n add r3, r1, r2\n sub r4, r1, r2\n mul r5, r1, r2\n div r6, r1, r2\n mod r7, r1, r2\n and r8, r1, r2\n or r9, r1, r2\n xor r10, r1, r2\n movi r11, 3\n shl r12, r1, r11\n shr r13, r1, r11\n not r14, r2\n neg r15, r2\n halt",
    );
    assert_eq!(vm.status, Status::Halted);
    assert_eq!(&vm.regs[3..8], &[107, 93, 700, 14, 2]);
    assert_eq!(&vm.regs[8..11], &[100 & 7, 100 | 7, 100 ^ 7]);
    assert_eq!(vm.regs[12], 800);
    assert_eq!(vm.regs[13], 12);
    assert_eq!(vm.regs[14], !7);
    assert_eq!(vm.regs[15], -7);
}

#[test]
fn wrapping_and_signed_semantics() {
    let vm = run(
        "movi r1, 0x7fffffff\n muli r1, r1, 0x7fffffff\n muli r1, r1, 0x7fffffff\n movi r2, -1\n movi r3, 1\n shr r4, r2, r3\n movi r5, -7\n movi r6, 2\n div r7, r5, r6\n mod r8, r5, r6\n halt",
    );
    assert_eq!(
        vm.regs[1],
        0x7fff_ffffi64
            .wrapping_mul(0x7fff_ffff)
            .wrapping_mul(0x7fff_ffff)
    );
    assert_eq!(vm.regs[4], i64::MAX, "shr is logical");
    assert_eq!(vm.regs[7], -3, "div truncates toward zero");
    assert_eq!(vm.regs[8], -1);
}

#[test]
fn branches_and_loops() {
    // sum 1..=10 with a counted loop
    let vm = run(
        "movi r1, 0\n movi r2, 1\n movi r3, 11\n loop:\n jge r2, r3, done\n add r1, r1, r2\n addi r2, r2, 1\n jmp loop\n done:\n cmp r4, r1, r3\n halt",
    );
    assert_eq!(vm.regs[1], 55);
    assert_eq!(vm.regs[4], 1);
    let vm = run(
        "movi r1, 5\n movi r2, 5\n jne r1, r2, bad\n jeq r1, r2, ok\n bad:\n movi r0, -1\n halt\n ok:\n movi r0, 1\n jz r0, bad\n jnz r0, end\n movi r0, -2\n end:\n halt",
    );
    assert_eq!(vm.regs[0], 1);
}

#[test]
fn memory_words_and_bytes() {
    let vm = run(
        ".data buf 32\n .word answer 42\n movi r1, buf\n movi r2, -123456789\n st r2, r1, 8\n ld r3, r1, 8\n movi r4, 200\n stb r4, r1, 3\n ldb r5, r1, 3\n movi r6, answer\n ld r7, r6, 0\n halt",
    );
    assert_eq!(vm.regs[3], -123456789);
    assert_eq!(vm.regs[5], 200);
    assert_eq!(vm.regs[7], 42);
    assert_eq!(vm.mem[3], 200);
}

#[test]
fn calls_stack_and_recursion() {
    // fib(20) recursively, using the data stack for the frame
    let src = "movi r1, 20\n call fib\n halt\n\
        fib:\n movi r2, 2\n jlt r1, r2, base\n push r1\n addi r1, r1, -1\n call fib\n pop r1\n push r0\n addi r1, r1, -2\n call fib\n pop r2\n add r0, r0, r2\n ret\n\
        base:\n mov r0, r1\n ret";
    let vm = run(src);
    assert_eq!(vm.status, Status::Halted);
    assert_eq!(vm.regs[0], 6765);
    assert!(vm.stack.is_empty() && vm.calls.is_empty(), "balanced");
}

#[test]
fn traps_are_catchable_with_seth() {
    let vm = run(
        "seth handler\n movi r1, 1\n movi r2, 0\n div r3, r1, r2\n movi r0, 99\n halt\n handler:\n mov r4, r0\n movi r0, 7\n halt",
    );
    assert_eq!(vm.status, Status::Halted);
    assert_eq!(vm.regs[4], trap::DIV_ZERO);
    assert_eq!(vm.regs[0], 7);
    // user traps carry their code; the handler is one-shot
    let vm = run("seth h\n trap 40\n h:\n mov r5, r0\n trap 41\n halt");
    assert_eq!(vm.regs[5], 40);
    assert_eq!(vm.status, Status::Trapped(41));
}

#[test]
fn uncaught_traps_stop_the_machine() {
    assert_eq!(
        run("movi r1, 1\n movi r2, 0\n mod r3, r1, r2").status,
        Status::Trapped(trap::DIV_ZERO)
    );
    assert_eq!(
        run("movi r1, -1\n ld r2, r1, 0").status,
        Status::Trapped(trap::MEM_OOB)
    );
    assert_eq!(
        run("movi r1, 65536\n ldb r2, r1, 0").status,
        Status::Trapped(trap::MEM_OOB)
    );
    assert_eq!(
        run("movi r1, 65535\n ld r2, r1, 0").status,
        Status::Trapped(trap::MEM_OOB),
        "8-byte read straddling the end"
    );
    assert_eq!(run("pop r1").status, Status::Trapped(trap::STACK_UNDERFLOW));
    assert_eq!(run("ret").status, Status::Trapped(trap::STACK_UNDERFLOW));
    assert_eq!(run("jmp 999").status, Status::Trapped(trap::BAD_JUMP));
    assert_eq!(run("sys 77").status, Status::Trapped(trap::BAD_SYSCALL));
    let mut p = asm::assemble("nop").unwrap();
    p.code[0].op = 250;
    let mut vm = Vm::new(&p, Config::default());
    assert_eq!(vm.run(10), Status::Trapped(trap::ILLEGAL));
}

#[test]
fn heap_allocation() {
    let vm =
        run("movi r1, 100\n alloc r2, r1\n alloc r3, r1\n movi r4, 65536\n alloc r5, r4\n halt");
    assert_eq!(vm.status, Status::Trapped(trap::OOM));
    assert_eq!(vm.regs[3] - vm.regs[2], 104, "8-byte aligned");
    assert_eq!(vm.regs[2] % 8, 0);
}

#[test]
fn syscalls_write_and_ticks_and_rand() {
    let vm = run(
        ".data msg \"hi\\n\"\n movi r1, msg\n movi r2, 3\n sys write\n sys ticks\n mov r5, r0\n sys rand\n mov r6, r0\n sys rand\n mov r7, r0\n halt",
    );
    assert_eq!(vm.output, b"hi\n");
    assert!(vm.regs[5] > 0);
    assert_ne!(vm.regs[6], vm.regs[7]);
    assert!(vm.regs[6] >= 0 && vm.regs[7] >= 0);
}

#[test]
fn running_off_the_end_halts_cleanly() {
    let vm = run("movi r1, 3");
    assert_eq!(vm.status, Status::Halted);
    assert_eq!(vm.regs[1], 3);
}
