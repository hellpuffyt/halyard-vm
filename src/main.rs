//! `halyard` — assemble, run, disassemble and debug Halyard programs.
//!
//! ```text
//! halyard asm  prog.hasm -o prog.hy
//! halyard run  prog.hy|prog.hasm [--gas N] [--seed N] [--input FILE] [--cap output,emit,input,host:ID] [--trace] [--hash]
//! halyard dis  prog.hy
//! halyard debug prog.hy|prog.hasm      interactive: s[tep] [n], c[ontinue], r[egs], m[em] addr len, b[reak] pc, d[is], q[uit]
//! ```

use std::io::{BufRead, Write};

use halyard::{Cap, Config, Program, Status, Vm, asm};

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1)
}

fn load(path: &str) -> Program {
    let bytes = std::fs::read(path).unwrap_or_else(|e| die(format!("{path}: {e}")));
    if path.ends_with(".hasm") {
        asm::assemble(&String::from_utf8_lossy(&bytes))
            .unwrap_or_else(|e| die(format!("{path}: {e}")))
    } else {
        Program::from_bytes(&bytes).unwrap_or_else(|e| die(format!("{path}: {e}")))
    }
}

struct Opts {
    gas: u64,
    cfg: Config,
    hash: bool,
}

fn parse_opts(args: &[String]) -> Opts {
    let mut o = Opts {
        gas: 10_000_000,
        cfg: Config::default(),
        hash: false,
    };
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| -> &String {
            *i += 1;
            args.get(*i)
                .unwrap_or_else(|| die(format!("{} needs a value", args[*i - 1])))
        };
        match args[i].as_str() {
            "--gas" => o.gas = next(&mut i).parse().unwrap_or_else(|_| die("bad --gas")),
            "--seed" => o.cfg.seed = next(&mut i).parse().unwrap_or_else(|_| die("bad --seed")),
            "--memory" => {
                o.cfg.memory_size = next(&mut i).parse().unwrap_or_else(|_| die("bad --memory"))
            }
            "--input" => {
                let p = next(&mut i);
                o.cfg.input = std::fs::read(p).unwrap_or_else(|e| die(format!("{p}: {e}")));
                if !o.cfg.caps.contains(&Cap::Input) {
                    o.cfg.caps.push(Cap::Input);
                }
            }
            "--cap" => {
                o.cfg.caps = next(&mut i)
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|c| match c {
                        "output" => Cap::Output,
                        "input" => Cap::Input,
                        "emit" => Cap::Emit,
                        h => Cap::Host(
                            h.strip_prefix("host:")
                                .and_then(|n| n.parse().ok())
                                .unwrap_or_else(|| die(format!("unknown capability {c}"))),
                        ),
                    })
                    .collect();
            }
            "--trace" => o.cfg.trace = true,
            "--hash" => o.hash = true,
            other => die(format!("unknown option {other}")),
        }
        i += 1;
    }
    o
}

fn report(vm: &Vm, status: Status, hash: bool) -> i32 {
    std::io::stdout().write_all(&vm.output).ok();
    for e in &vm.events {
        eprintln!("event: {}", String::from_utf8_lossy(e));
    }
    eprintln!("[{status}; gas used {}; r0 = {}]", vm.gas_used, vm.regs[0]);
    if hash {
        println!("state hash: {:016x}", vm.state_hash());
    }
    match status {
        Status::Halted => 0,
        Status::OutOfGas => 3,
        Status::Trapped(_) => 4,
        Status::Running => 5,
    }
}

fn debug(prog: Program, opts: Opts) -> i32 {
    let mut vm = Vm::new(&prog, opts.cfg);
    let mut breaks: Vec<u32> = Vec::new();
    let stdin = std::io::stdin();
    let show = |vm: &Vm| {
        let pc = vm.pc as usize;
        if pc < prog.code.len() {
            println!("{pc:04}  {}", asm::format_instr(prog.code[pc]));
        } else {
            println!("{pc:04}  <end of code>");
        }
    };
    println!(
        "Halyard debugger — {} instructions, {} data bytes. Type h for help.",
        prog.code.len(),
        prog.data.len()
    );
    show(&vm);
    loop {
        print!("(hy) ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.first().copied().unwrap_or("") {
            "" => {}
            "q" | "quit" => break,
            "h" | "help" => println!(
                "s [n]   step n instructions\nc       continue to breakpoint/halt/trap\nr       registers\nm A N   dump N bytes at A\nb PC    toggle breakpoint\nd       disassemble around pc\nst      data stack + call stack\nhash    state hash\nq       quit"
            ),
            "s" | "step" => {
                let n: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                for _ in 0..n {
                    if vm.step() != Status::Running {
                        break;
                    }
                }
                println!("gas {}  status {}", vm.gas_used, vm.status);
                show(&vm);
            }
            "c" | "continue" => {
                loop {
                    let st = vm.step();
                    if st != Status::Running {
                        println!("stopped: {st} (gas {})", vm.gas_used);
                        break;
                    }
                    if breaks.contains(&vm.pc) {
                        println!("breakpoint at {:04}", vm.pc);
                        break;
                    }
                    if vm.gas_used >= opts.gas {
                        println!("gas limit {} reached", opts.gas);
                        break;
                    }
                }
                show(&vm);
            }
            "r" | "regs" => {
                for (i, r) in vm.regs.iter().enumerate() {
                    print!("r{i:<2}={r:<12}");
                    if i % 4 == 3 {
                        println!();
                    }
                }
                println!(
                    "pc={} heap={} handler={:?} gas={}",
                    vm.pc, vm.heap, vm.handler, vm.gas_used
                );
            }
            "m" | "mem" => {
                let a: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let n: usize = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);
                let end = (a + n).min(vm.mem.len());
                for (off, chunk) in vm.mem[a.min(end)..end].chunks(16).enumerate() {
                    let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                    let txt: String = chunk
                        .iter()
                        .map(|&b| {
                            if (32..127).contains(&b) {
                                b as char
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    println!("{:06x}  {:<48} {txt}", a + off * 16, hex.join(" "));
                }
            }
            "b" | "break" => match parts.get(1).and_then(|s| s.parse::<u32>().ok()) {
                Some(pc) => {
                    if let Some(i) = breaks.iter().position(|&b| b == pc) {
                        breaks.remove(i);
                        println!("breakpoint {pc} removed");
                    } else {
                        breaks.push(pc);
                        println!("breakpoint {pc} set");
                    }
                }
                None => println!("usage: b PC"),
            },
            "d" | "dis" => {
                let pc = vm.pc as usize;
                let lo = pc.saturating_sub(4);
                for (i, ins) in prog.code.iter().enumerate().skip(lo).take(12) {
                    println!(
                        "{}{i:04}  {}",
                        if i == pc { "=>" } else { "  " },
                        asm::format_instr(*ins)
                    );
                }
            }
            "st" => println!("stack {:?}\ncalls {:?}", vm.stack, vm.calls),
            "hash" => println!("{:016x}", vm.state_hash()),
            other => println!("unknown command {other} (h for help)"),
        }
    }
    let st = vm.status;
    report(&vm, st, false)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, file) = match (args.first(), args.get(1)) {
        (Some(c), Some(f)) => (c.as_str(), f.as_str()),
        _ => {
            eprintln!(
                "usage: halyard <asm|run|dis|debug> <file> [options]\n  asm  prog.hasm -o prog.hy\n  run  prog.hy [--gas N] [--seed N] [--input FILE] [--cap output,emit,input,host:ID] [--trace] [--hash]\n  dis  prog.hy\n  debug prog.hy"
            );
            std::process::exit(2);
        }
    };
    let code = match cmd {
        "asm" => {
            let src = std::fs::read_to_string(file).unwrap_or_else(|e| die(format!("{file}: {e}")));
            let prog = asm::assemble(&src).unwrap_or_else(|e| die(format!("{file}: {e}")));
            let out = match args.get(2).map(String::as_str) {
                Some("-o") => args
                    .get(3)
                    .cloned()
                    .unwrap_or_else(|| die("-o needs a path")),
                _ => file.trim_end_matches(".hasm").to_string() + ".hy",
            };
            std::fs::write(&out, prog.to_bytes()).unwrap_or_else(|e| die(format!("{out}: {e}")));
            eprintln!(
                "wrote {out}: {} instructions, {} data bytes",
                prog.code.len(),
                prog.data.len()
            );
            0
        }
        "dis" => {
            print!("{}", asm::disassemble(&load(file)));
            0
        }
        "run" => {
            let opts = parse_opts(&args[2..]);
            let prog = load(file);
            let hash = opts.hash;
            let mut vm = Vm::new(&prog, opts.cfg);
            let st = vm.run(opts.gas);
            if opts.gas > 0 && std::env::var_os("HALYARD_TRACE").is_some() {
                eprintln!("trace: {:?}", vm.trace);
            }
            report(&vm, st, hash)
        }
        "debug" => {
            let opts = parse_opts(&args[2..]);
            debug(load(file), opts)
        }
        other => die(format!("unknown command {other}")),
    };
    std::process::exit(code);
}
