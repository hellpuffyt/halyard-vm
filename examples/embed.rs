//! Embedding Halyard: a host exposes one capability-gated function to an
//! untrusted program, runs it under a gas budget, and inspects the result.
//!
//! `cargo run --example embed`

use halyard::{Cap, Config, Status, Vm, asm};

fn main() {
    // The "plugin": asks the host (function 1) to look up prices for three
    // SKUs and sums them. It cannot do anything else — no output, no input.
    let plugin = asm::assemble(
        "movi r5, 0\n movi r6, 3\n movi r7, 0\n\
         loop:\n jge r5, r6, done\n movi r0, 1\n mov r1, r5\n sys host\n add r7, r7, r0\n addi r5, r5, 1\n jmp loop\n\
         done:\n mov r0, r7\n halt",
    )
    .unwrap();

    let prices = [1999_i64, 250, 4500];
    let mut vm = Vm::new(
        &plugin,
        Config {
            caps: vec![Cap::Host(1)],
            ..Default::default()
        },
    );
    vm.register_host(
        1,
        Box::new(move |_mem, args| {
            prices
                .get(args[0] as usize)
                .copied()
                .ok_or_else(|| format!("unknown sku {}", args[0]))
        }),
    );

    match vm.run(10_000) {
        Status::Halted => println!("plugin returned {} (gas used {})", vm.regs[0], vm.gas_used),
        other => println!("plugin stopped: {other}"),
    }
    assert_eq!(vm.regs[0], 6749);

    // Same plugin, no capability: the host call traps instead of running.
    let mut locked = Vm::new(&plugin, Config::default());
    println!("without Cap::Host(1): {}", locked.run(10_000));
    println!("state hash: {:016x}", vm.state_hash());
}
