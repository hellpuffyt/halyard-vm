//! Halyard — a deterministic, gas-metered, capability-gated register VM for
//! running untrusted automation (agent tools, workflow steps, plugins).
//!
//! Design in one paragraph: 16 signed 64-bit registers, a Harvard layout
//! (code is not in data memory, so no self-modifying code), a bounded data
//! stack and a bounded call stack, a bump-allocated heap inside a fixed
//! memory arena, fixed 8-byte instructions, a trap model with an optional
//! in-program handler, gas accounting on every instruction, and host access
//! only through numbered syscalls behind explicit capabilities. Every run
//! with the same program, input, seed and gas is bit-for-bit identical —
//! [`Vm::state_hash`] lets you prove it, and [`Vm::snapshot`] /
//! [`Vm::restore`] let you pause, persist and resume anywhere.
//!
//! ```
//! use halyard::{Program, Vm, Status, asm};
//! let prog = asm::assemble("movi r1, 6\n movi r2, 7\n mul r0, r1, r2\n halt").unwrap();
//! let mut vm = Vm::new(&prog, Default::default());
//! assert_eq!(vm.run(1_000), Status::Halted);
//! assert_eq!(vm.regs[0], 42);
//! ```

pub mod asm;

use std::collections::HashMap;
use std::fmt;

/// Fixed-width instruction: opcode, three register operands, a 32-bit
/// immediate. Encoded as 8 little-endian bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Instr {
    pub op: u8,
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub imm: i32,
}

impl Instr {
    pub const fn new(op: Op, a: u8, b: u8, c: u8, imm: i32) -> Self {
        Instr {
            op: op as u8,
            a,
            b,
            c,
            imm,
        }
    }
    pub fn to_bytes(self) -> [u8; 8] {
        let i = self.imm.to_le_bytes();
        [self.op, self.a, self.b, self.c, i[0], i[1], i[2], i[3]]
    }
    pub fn from_bytes(b: [u8; 8]) -> Self {
        Instr {
            op: b[0],
            a: b[1],
            b: b[2],
            c: b[3],
            imm: i32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        }
    }
}

macro_rules! ops {
    ($($name:ident = $v:expr, $mn:literal, $fmt:literal;)*) => {
        /// The instruction set. `fmt` letters: a/b/c register operands, i immediate.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        pub enum Op { $($name = $v,)* }
        impl Op {
            pub fn from_u8(v: u8) -> Option<Op> {
                match v { $($v => Some(Op::$name),)* _ => None }
            }
            pub fn mnemonic(self) -> &'static str {
                match self { $(Op::$name => $mn,)* }
            }
            /// Operand shape, e.g. "abc", "ai", "i", "".
            pub fn shape(self) -> &'static str {
                match self { $(Op::$name => $fmt,)* }
            }
            pub fn from_mnemonic(s: &str) -> Option<Op> {
                match s { $($mn => Some(Op::$name),)* _ => None }
            }
        }
    };
}

ops! {
    Nop   = 0,  "nop",   "";
    Halt  = 1,  "halt",  "";
    Movi  = 2,  "movi",  "ai";
    Mov   = 3,  "mov",   "ab";
    Add   = 4,  "add",   "abc";
    Sub   = 5,  "sub",   "abc";
    Mul   = 6,  "mul",   "abc";
    Div   = 7,  "div",   "abc";
    Mod   = 8,  "mod",   "abc";
    And   = 9,  "and",   "abc";
    Or    = 10, "or",    "abc";
    Xor   = 11, "xor",   "abc";
    Shl   = 12, "shl",   "abc";
    Shr   = 13, "shr",   "abc";
    Addi  = 14, "addi",  "abi";
    Cmp   = 15, "cmp",   "abc";
    Jmp   = 16, "jmp",   "i";
    Jz    = 17, "jz",    "ai";
    Jnz   = 18, "jnz",   "ai";
    Jlt   = 19, "jlt",   "abi";
    Jge   = 20, "jge",   "abi";
    Jeq   = 21, "jeq",   "abi";
    Jne   = 22, "jne",   "abi";
    Ld    = 23, "ld",    "abi";
    St    = 24, "st",    "abi";
    Ldb   = 25, "ldb",   "abi";
    Stb   = 26, "stb",   "abi";
    Push  = 27, "push",  "a";
    Pop   = 28, "pop",   "a";
    Call  = 29, "call",  "i";
    Ret   = 30, "ret",   "";
    Alloc = 31, "alloc", "ab";
    Sys   = 32, "sys",   "i";
    Trap  = 33, "trap",  "i";
    Seth  = 34, "seth",  "i";
    Not   = 35, "not",   "ab";
    Neg   = 36, "neg",   "ab";
    Muli  = 37, "muli",  "abi";
}

/// Syscall numbers (the `sys` immediate).
pub mod sys {
    /// Append `mem[r1 .. r1+r2]` to the VM's output. Needs `Cap::Output`.
    pub const WRITE: i32 = 1;
    /// `r0 = gas used so far` — a deterministic clock.
    pub const TICKS: i32 = 2;
    /// `r0 = next value` of the run's seeded PRNG (deterministic).
    pub const RAND: i32 = 3;
    /// Call host function `r0` with `(r1, r2, r3)`; result in `r0`.
    /// Needs `Cap::Host(id)`.
    pub const HOST: i32 = 4;
    /// Emit an event: `mem[r1 .. r1+r2]` is appended to `Vm::events`.
    /// Needs `Cap::Emit`.
    pub const EMIT: i32 = 5;
    /// `r0 = number of input bytes`; input is at `mem[r1..]` after `READ`.
    /// Copies the host-provided input into `mem[r1 .. r1+len]`, truncated to
    /// `r2` bytes; `r0 = bytes copied`. Needs `Cap::Input`.
    pub const READ: i32 = 6;

    pub fn name(n: i32) -> Option<&'static str> {
        Some(match n {
            WRITE => "write",
            TICKS => "ticks",
            RAND => "rand",
            HOST => "host",
            EMIT => "emit",
            READ => "read",
            _ => return None,
        })
    }
    pub fn from_name(s: &str) -> Option<i32> {
        Some(match s {
            "write" => WRITE,
            "ticks" => TICKS,
            "rand" => RAND,
            "host" => HOST,
            "emit" => EMIT,
            "read" => READ,
            _ => return None,
        })
    }
}

/// Trap codes delivered to the handler in `r0` (or reported as
/// `Status::Trapped`). User traps (`trap n`) use `n` directly; keep them ≥ 32.
pub mod trap {
    pub const DIV_ZERO: i64 = 1;
    pub const MEM_OOB: i64 = 2;
    pub const STACK_OVERFLOW: i64 = 3;
    pub const STACK_UNDERFLOW: i64 = 4;
    pub const ILLEGAL: i64 = 5;
    pub const OOM: i64 = 6;
    pub const CAP_DENIED: i64 = 7;
    pub const BAD_SYSCALL: i64 = 8;
    pub const HOST_ERROR: i64 = 9;
    pub const BAD_JUMP: i64 = 10;

    pub fn name(c: i64) -> String {
        match c {
            DIV_ZERO => "division by zero".into(),
            MEM_OOB => "memory access out of bounds".into(),
            STACK_OVERFLOW => "stack overflow".into(),
            STACK_UNDERFLOW => "stack underflow".into(),
            ILLEGAL => "illegal instruction".into(),
            OOM => "out of memory".into(),
            CAP_DENIED => "capability denied".into(),
            BAD_SYSCALL => "bad syscall".into(),
            HOST_ERROR => "host function error".into(),
            BAD_JUMP => "jump target out of range".into(),
            n => format!("user trap {n}"),
        }
    }
}

/// A loadable program: code segment + initial data segment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    pub code: Vec<Instr>,
    pub data: Vec<u8>,
}

const MAGIC: &[u8; 4] = b"HLYD";

impl Program {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(16 + self.code.len() * 8 + self.data.len());
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&(self.code.len() as u32).to_le_bytes());
        b.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        for i in &self.code {
            b.extend_from_slice(&i.to_bytes());
        }
        b.extend_from_slice(&self.data);
        b
    }
    pub fn from_bytes(b: &[u8]) -> Result<Program, String> {
        if b.len() < 16 || &b[..4] != MAGIC {
            return Err("not a Halyard program (bad magic)".into());
        }
        let ver = u32::from_le_bytes(b[4..8].try_into().unwrap());
        if ver != 1 {
            return Err(format!("unsupported program version {ver}"));
        }
        let ncode = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        let ndata = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
        let need = 16usize
            .checked_add(ncode.checked_mul(8).ok_or("size overflow")?)
            .and_then(|n| n.checked_add(ndata))
            .ok_or("size overflow")?;
        if b.len() != need {
            return Err(format!(
                "truncated program: expected {need} bytes, got {}",
                b.len()
            ));
        }
        let code = b[16..16 + ncode * 8]
            .chunks_exact(8)
            .map(|c| Instr::from_bytes(c.try_into().unwrap()))
            .collect();
        Ok(Program {
            code,
            data: b[16 + ncode * 8..].to_vec(),
        })
    }
}

/// What a program may touch outside its own memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Cap {
    Output,
    Input,
    Emit,
    Host(i64),
}

pub type HostFn = Box<dyn FnMut(&mut [u8], [i64; 3]) -> Result<i64, String>>;

/// Per-run limits and inputs.
pub struct Config {
    pub memory_size: usize,
    pub max_stack: usize,
    pub max_call_depth: usize,
    pub seed: u64,
    pub input: Vec<u8>,
    pub caps: Vec<Cap>,
    pub trace: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            memory_size: 64 * 1024,
            max_stack: 1024,
            max_call_depth: 256,
            seed: 0,
            input: Vec::new(),
            caps: vec![Cap::Output],
            trace: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Halted,
    /// Gas ran out; call `run` again with more gas to continue.
    OutOfGas,
    /// A trap with no handler installed. The VM cannot continue.
    Trapped(i64),
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Running => write!(f, "running"),
            Status::Halted => write!(f, "halted"),
            Status::OutOfGas => write!(f, "out of gas"),
            Status::Trapped(c) => write!(f, "trapped: {}", trap::name(*c)),
        }
    }
}

pub struct Vm {
    pub regs: [i64; 16],
    pub pc: u32,
    pub mem: Vec<u8>,
    pub stack: Vec<i64>,
    pub calls: Vec<u32>,
    pub heap: usize,
    pub handler: Option<u32>,
    pub gas_used: u64,
    pub output: Vec<u8>,
    pub events: Vec<Vec<u8>>,
    pub trace: Vec<u32>,
    pub status: Status,
    code: Vec<Instr>,
    rng: u64,
    input: Vec<u8>,
    caps: Vec<Cap>,
    hosts: HashMap<i64, HostFn>,
    max_stack: usize,
    max_call_depth: usize,
    tracing: bool,
}

/// Snapshot format tag.
const SNAP_MAGIC: &[u8; 4] = b"HSNP";

impl Vm {
    pub fn new(program: &Program, cfg: Config) -> Vm {
        let mut mem = vec![0u8; cfg.memory_size.max(program.data.len())];
        mem[..program.data.len()].copy_from_slice(&program.data);
        let heap = program.data.len().div_ceil(8) * 8;
        Vm {
            regs: [0; 16],
            pc: 0,
            mem,
            stack: Vec::new(),
            calls: Vec::new(),
            heap,
            handler: None,
            gas_used: 0,
            output: Vec::new(),
            events: Vec::new(),
            trace: Vec::new(),
            status: Status::Running,
            code: program.code.clone(),
            rng: cfg.seed ^ 0x9E37_79B9_7F4A_7C15,
            input: cfg.input,
            caps: cfg.caps,
            hosts: HashMap::new(),
            max_stack: cfg.max_stack,
            max_call_depth: cfg.max_call_depth,
            tracing: cfg.trace,
        }
    }

    /// Registers a host function callable via `sys host` with `r0 = id`.
    /// The program still needs `Cap::Host(id)` to call it.
    pub fn register_host(&mut self, id: i64, f: HostFn) {
        self.hosts.insert(id, f);
    }

    pub fn code(&self) -> &[Instr] {
        &self.code
    }

    /// Runs until halt, trap, or `gas` more units are consumed.
    pub fn run(&mut self, gas: u64) -> Status {
        let limit = self.gas_used.saturating_add(gas);
        while self.status == Status::Running {
            if self.gas_used >= limit {
                self.status = Status::OutOfGas;
                break;
            }
            self.step();
        }
        self.status
    }

    /// Continues a VM that stopped with `OutOfGas`.
    pub fn resume(&mut self, gas: u64) -> Status {
        if self.status == Status::OutOfGas {
            self.status = Status::Running;
        }
        self.run(gas)
    }

    fn raise(&mut self, code: i64) {
        match self.handler {
            Some(h) if (h as usize) < self.code.len() => {
                self.regs[0] = code;
                self.pc = h;
                self.handler = None; // one-shot: handler must re-arm with `seth`
            }
            _ => self.status = Status::Trapped(code),
        }
    }

    fn addr(&self, base: i64, imm: i32, len: usize) -> Option<usize> {
        let a = base.checked_add(imm as i64)?;
        if a < 0 {
            return None;
        }
        let a = a as usize;
        (a.checked_add(len)? <= self.mem.len()).then_some(a)
    }

    fn jump(&mut self, target: i32) {
        if target < 0 || target as usize > self.code.len() {
            self.raise(trap::BAD_JUMP);
        } else {
            self.pc = target as u32;
        }
    }

    /// Executes one instruction.
    pub fn step(&mut self) -> Status {
        if self.status != Status::Running {
            return self.status;
        }
        let pc = self.pc as usize;
        if pc >= self.code.len() {
            self.status = Status::Halted; // running off the end is a clean halt
            return self.status;
        }
        let i = self.code[pc];
        if self.tracing {
            self.trace.push(self.pc);
        }
        self.pc += 1;
        let Some(op) = Op::from_u8(i.op) else {
            self.gas_used += 1;
            self.raise(trap::ILLEGAL);
            return self.status;
        };
        let (a, b, c) = (i.a as usize & 15, i.b as usize & 15, i.c as usize & 15);
        if i.a > 15 || i.b > 15 || i.c > 15 {
            self.gas_used += 1;
            self.raise(trap::ILLEGAL);
            return self.status;
        }
        self.gas_used += match op {
            Op::Mul | Op::Div | Op::Mod | Op::Muli => 3,
            Op::Ld | Op::St | Op::Ldb | Op::Stb | Op::Push | Op::Pop | Op::Call | Op::Ret => 2,
            Op::Alloc => 5,
            Op::Sys => 10,
            _ => 1,
        };
        match op {
            Op::Nop => {}
            Op::Halt => self.status = Status::Halted,
            Op::Movi => self.regs[a] = i.imm as i64,
            Op::Mov => self.regs[a] = self.regs[b],
            Op::Add => self.regs[a] = self.regs[b].wrapping_add(self.regs[c]),
            Op::Sub => self.regs[a] = self.regs[b].wrapping_sub(self.regs[c]),
            Op::Mul => self.regs[a] = self.regs[b].wrapping_mul(self.regs[c]),
            Op::Muli => self.regs[a] = self.regs[b].wrapping_mul(i.imm as i64),
            Op::Div | Op::Mod => {
                if self.regs[c] == 0 {
                    self.raise(trap::DIV_ZERO);
                } else if op == Op::Div {
                    self.regs[a] = self.regs[b].wrapping_div(self.regs[c]);
                } else {
                    self.regs[a] = self.regs[b].wrapping_rem(self.regs[c]);
                }
            }
            Op::And => self.regs[a] = self.regs[b] & self.regs[c],
            Op::Or => self.regs[a] = self.regs[b] | self.regs[c],
            Op::Xor => self.regs[a] = self.regs[b] ^ self.regs[c],
            Op::Shl => self.regs[a] = self.regs[b].wrapping_shl(self.regs[c] as u32 & 63),
            Op::Shr => self.regs[a] = ((self.regs[b] as u64) >> (self.regs[c] as u32 & 63)) as i64,
            Op::Not => self.regs[a] = !self.regs[b],
            Op::Neg => self.regs[a] = self.regs[b].wrapping_neg(),
            Op::Addi => self.regs[a] = self.regs[b].wrapping_add(i.imm as i64),
            Op::Cmp => {
                self.regs[a] =
                    (self.regs[b] > self.regs[c]) as i64 - (self.regs[b] < self.regs[c]) as i64
            }
            Op::Jmp => self.jump(i.imm),
            Op::Jz => {
                if self.regs[a] == 0 {
                    self.jump(i.imm)
                }
            }
            Op::Jnz => {
                if self.regs[a] != 0 {
                    self.jump(i.imm)
                }
            }
            Op::Jlt => {
                if self.regs[a] < self.regs[b] {
                    self.jump(i.imm)
                }
            }
            Op::Jge => {
                if self.regs[a] >= self.regs[b] {
                    self.jump(i.imm)
                }
            }
            Op::Jeq => {
                if self.regs[a] == self.regs[b] {
                    self.jump(i.imm)
                }
            }
            Op::Jne => {
                if self.regs[a] != self.regs[b] {
                    self.jump(i.imm)
                }
            }
            Op::Ld => match self.addr(self.regs[b], i.imm, 8) {
                Some(p) => {
                    self.regs[a] = i64::from_le_bytes(self.mem[p..p + 8].try_into().unwrap())
                }
                None => self.raise(trap::MEM_OOB),
            },
            Op::St => match self.addr(self.regs[b], i.imm, 8) {
                Some(p) => self.mem[p..p + 8].copy_from_slice(&self.regs[a].to_le_bytes()),
                None => self.raise(trap::MEM_OOB),
            },
            Op::Ldb => match self.addr(self.regs[b], i.imm, 1) {
                Some(p) => self.regs[a] = self.mem[p] as i64,
                None => self.raise(trap::MEM_OOB),
            },
            Op::Stb => match self.addr(self.regs[b], i.imm, 1) {
                Some(p) => self.mem[p] = self.regs[a] as u8,
                None => self.raise(trap::MEM_OOB),
            },
            Op::Push => {
                if self.stack.len() >= self.max_stack {
                    self.raise(trap::STACK_OVERFLOW);
                } else {
                    self.stack.push(self.regs[a]);
                }
            }
            Op::Pop => match self.stack.pop() {
                Some(v) => self.regs[a] = v,
                None => self.raise(trap::STACK_UNDERFLOW),
            },
            Op::Call => {
                if self.calls.len() >= self.max_call_depth {
                    self.raise(trap::STACK_OVERFLOW);
                } else {
                    self.calls.push(self.pc);
                    self.jump(i.imm);
                }
            }
            Op::Ret => match self.calls.pop() {
                Some(p) => self.pc = p,
                None => self.raise(trap::STACK_UNDERFLOW),
            },
            Op::Alloc => {
                let n = self.regs[b];
                if n < 0 {
                    self.raise(trap::OOM);
                } else {
                    let size = (n as usize).div_ceil(8) * 8;
                    match self.heap.checked_add(size) {
                        Some(end) if end <= self.mem.len() => {
                            self.regs[a] = self.heap as i64;
                            self.heap = end;
                        }
                        _ => self.raise(trap::OOM),
                    }
                }
            }
            Op::Trap => self.raise(i.imm as i64),
            Op::Seth => {
                self.handler = if i.imm < 0 || i.imm as usize >= self.code.len() {
                    None
                } else {
                    Some(i.imm as u32)
                }
            }
            Op::Sys => self.syscall(i.imm),
        }
        self.status
    }

    fn has_cap(&self, c: &Cap) -> bool {
        self.caps.contains(c)
    }

    fn syscall(&mut self, n: i32) {
        match n {
            sys::WRITE | sys::EMIT => {
                let cap = if n == sys::WRITE {
                    Cap::Output
                } else {
                    Cap::Emit
                };
                if !self.has_cap(&cap) {
                    return self.raise(trap::CAP_DENIED);
                }
                let (ptr, len) = (self.regs[1], self.regs[2]);
                if len < 0 {
                    return self.raise(trap::MEM_OOB);
                }
                match self.addr(ptr, 0, len as usize) {
                    Some(p) => {
                        let bytes = self.mem[p..p + len as usize].to_vec();
                        if n == sys::WRITE {
                            self.output.extend_from_slice(&bytes);
                        } else {
                            self.events.push(bytes);
                        }
                        self.regs[0] = len;
                    }
                    None => self.raise(trap::MEM_OOB),
                }
            }
            sys::TICKS => self.regs[0] = self.gas_used as i64,
            sys::RAND => {
                // xorshift64*: deterministic for a given seed.
                let mut x = self.rng;
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                self.rng = x;
                self.regs[0] = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 1) as i64;
            }
            sys::READ => {
                if !self.has_cap(&Cap::Input) {
                    return self.raise(trap::CAP_DENIED);
                }
                let (ptr, max) = (self.regs[1], self.regs[2]);
                let n = self.input.len().min(max.max(0) as usize);
                match self.addr(ptr, 0, n) {
                    Some(p) => {
                        self.mem[p..p + n].copy_from_slice(&self.input[..n]);
                        self.regs[0] = n as i64;
                    }
                    None => self.raise(trap::MEM_OOB),
                }
            }
            sys::HOST => {
                let id = self.regs[0];
                if !self.has_cap(&Cap::Host(id)) {
                    return self.raise(trap::CAP_DENIED);
                }
                let args = [self.regs[1], self.regs[2], self.regs[3]];
                let Some(f) = self.hosts.get_mut(&id) else {
                    return self.raise(trap::BAD_SYSCALL);
                };
                match f(&mut self.mem, args) {
                    Ok(v) => self.regs[0] = v,
                    Err(_) => self.raise(trap::HOST_ERROR),
                }
            }
            _ => self.raise(trap::BAD_SYSCALL),
        }
    }

    /// FNV-1a over the complete machine state (registers, pc, memory, stacks,
    /// heap pointer, handler, gas, rng). Equal hashes ⇔ equal states.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |bytes: &[u8]| {
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for r in &self.regs {
            feed(&r.to_le_bytes());
        }
        feed(&self.pc.to_le_bytes());
        feed(&self.mem);
        for s in &self.stack {
            feed(&s.to_le_bytes());
        }
        for c in &self.calls {
            feed(&c.to_le_bytes());
        }
        feed(&(self.heap as u64).to_le_bytes());
        feed(&self.handler.map_or(u32::MAX, |h| h).to_le_bytes());
        feed(&self.gas_used.to_le_bytes());
        feed(&self.rng.to_le_bytes());
        h
    }

    /// Serialises the resumable state (everything except host functions,
    /// which cannot be serialised and must be re-registered).
    pub fn snapshot(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(SNAP_MAGIC);
        let put = |b: &mut Vec<u8>, bytes: &[u8]| {
            b.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            b.extend_from_slice(bytes);
        };
        for r in &self.regs {
            b.extend_from_slice(&r.to_le_bytes());
        }
        b.extend_from_slice(&self.pc.to_le_bytes());
        b.extend_from_slice(&(self.heap as u64).to_le_bytes());
        b.extend_from_slice(&self.handler.map_or(u32::MAX, |h| h).to_le_bytes());
        b.extend_from_slice(&self.gas_used.to_le_bytes());
        b.extend_from_slice(&self.rng.to_le_bytes());
        b.push(match self.status {
            Status::Running => 0,
            Status::Halted => 1,
            Status::OutOfGas => 2,
            Status::Trapped(_) => 3,
        });
        b.extend_from_slice(
            &match self.status {
                Status::Trapped(c) => c,
                _ => 0,
            }
            .to_le_bytes(),
        );
        put(&mut b, &self.mem);
        let stack: Vec<u8> = self.stack.iter().flat_map(|v| v.to_le_bytes()).collect();
        put(&mut b, &stack);
        let calls: Vec<u8> = self.calls.iter().flat_map(|v| v.to_le_bytes()).collect();
        put(&mut b, &calls);
        put(&mut b, &self.output);
        b.extend_from_slice(&(self.events.len() as u32).to_le_bytes());
        for e in &self.events {
            put(&mut b, e);
        }
        b
    }

    /// Restores a snapshot taken from a VM running the same program.
    pub fn restore(&mut self, snap: &[u8]) -> Result<(), String> {
        let mut pos = 0usize;
        let take = |pos: &mut usize, n: usize| -> Result<&[u8], String> {
            let s = snap.get(*pos..*pos + n).ok_or("truncated snapshot")?;
            *pos += n;
            Ok(s)
        };
        if take(&mut pos, 4)? != SNAP_MAGIC {
            return Err("not a Halyard snapshot".into());
        }
        let mut regs = [0i64; 16];
        for r in &mut regs {
            *r = i64::from_le_bytes(take(&mut pos, 8)?.try_into().unwrap());
        }
        let pc = u32::from_le_bytes(take(&mut pos, 4)?.try_into().unwrap());
        let heap = u64::from_le_bytes(take(&mut pos, 8)?.try_into().unwrap()) as usize;
        let handler = u32::from_le_bytes(take(&mut pos, 4)?.try_into().unwrap());
        let gas = u64::from_le_bytes(take(&mut pos, 8)?.try_into().unwrap());
        let rng = u64::from_le_bytes(take(&mut pos, 8)?.try_into().unwrap());
        let st = take(&mut pos, 1)?[0];
        let code = i64::from_le_bytes(take(&mut pos, 8)?.try_into().unwrap());
        let blob = |pos: &mut usize| -> Result<Vec<u8>, String> {
            let n = u32::from_le_bytes(take(pos, 4)?.try_into().unwrap()) as usize;
            Ok(take(pos, n)?.to_vec())
        };
        let mem = blob(&mut pos)?;
        let stack = blob(&mut pos)?;
        let calls = blob(&mut pos)?;
        let output = blob(&mut pos)?;
        let n_events = u32::from_le_bytes(take(&mut pos, 4)?.try_into().unwrap()) as usize;
        let mut events = Vec::with_capacity(n_events);
        for _ in 0..n_events {
            events.push(blob(&mut pos)?);
        }
        if pc as usize > self.code.len() || heap > mem.len() {
            return Err("snapshot does not match this program".into());
        }
        self.regs = regs;
        self.pc = pc;
        self.heap = heap;
        self.handler = (handler != u32::MAX).then_some(handler);
        self.gas_used = gas;
        self.rng = rng;
        self.status = match st {
            0 => Status::Running,
            1 => Status::Halted,
            2 => Status::OutOfGas,
            _ => Status::Trapped(code),
        };
        self.mem = mem;
        self.stack = stack
            .chunks_exact(8)
            .map(|c| i64::from_le_bytes(c.try_into().unwrap()))
            .collect();
        self.calls = calls
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        self.output = output;
        self.events = events;
        Ok(())
    }
}
