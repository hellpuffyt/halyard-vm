# Halyard ISA reference

## Machine

- 16 registers `r0`–`r15`, signed 64-bit. Convention: `r0` result, `r1`–`r3`
  syscall arguments; nothing is enforced.
- `pc` indexes instructions (not bytes). Jump targets are absolute indices.
- Memory: flat byte arena, little-endian, data segment at address 0, heap
  above it. Default 64 KiB.
- Data stack (`push`/`pop`) and call stack (`call`/`ret`) are separate and
  bounded (defaults 1024 and 256).

## Encoding

Every instruction is 8 bytes: `op:u8 a:u8 b:u8 c:u8 imm:i32(LE)`. Unused
fields are zero. Register fields must be 0–15 (else `ILLEGAL`).

## Instructions

Shape letters: `a b c` registers, `i` immediate. `[x]` means 8-byte memory
at address `x`, `[x]b` one byte. Unsigned shift amounts are masked to 0–63.

| Op | Mnemonic | Shape | Semantics | Gas |
|---:|---|---|---|---:|
| 0 | `nop` | | — | 1 |
| 1 | `halt` | | status = Halted | 1 |
| 2 | `movi` | a i | ra = imm | 1 |
| 3 | `mov` | a b | ra = rb | 1 |
| 4 | `add` | a b c | ra = rb + rc (wrapping) | 1 |
| 5 | `sub` | a b c | ra = rb − rc | 1 |
| 6 | `mul` | a b c | ra = rb × rc | 3 |
| 7 | `div` | a b c | ra = rb ÷ rc, truncating; rc = 0 ⇒ `DIV_ZERO` | 3 |
| 8 | `mod` | a b c | ra = rb rem rc (sign of rb); rc = 0 ⇒ `DIV_ZERO` | 3 |
| 9 | `and` | a b c | ra = rb & rc | 1 |
| 10 | `or` | a b c | ra = rb \| rc | 1 |
| 11 | `xor` | a b c | ra = rb ^ rc | 1 |
| 12 | `shl` | a b c | ra = rb << (rc & 63) | 1 |
| 13 | `shr` | a b c | ra = rb >>> (rc & 63) (logical) | 1 |
| 14 | `addi` | a b i | ra = rb + imm | 1 |
| 15 | `cmp` | a b c | ra = −1 / 0 / 1 as rb <, =, > rc | 1 |
| 16 | `jmp` | i | pc = imm | 1 |
| 17 | `jz` | a i | if ra = 0: pc = imm | 1 |
| 18 | `jnz` | a i | if ra ≠ 0: pc = imm | 1 |
| 19 | `jlt` | a b i | if ra < rb: pc = imm | 1 |
| 20 | `jge` | a b i | if ra ≥ rb: pc = imm | 1 |
| 21 | `jeq` | a b i | if ra = rb: pc = imm | 1 |
| 22 | `jne` | a b i | if ra ≠ rb: pc = imm | 1 |
| 23 | `ld` | a b i | ra = [rb + imm] | 2 |
| 24 | `st` | a b i | [rb + imm] = ra | 2 |
| 25 | `ldb` | a b i | ra = [rb + imm]b (zero-extended) | 2 |
| 26 | `stb` | a b i | [rb + imm]b = ra & 0xFF | 2 |
| 27 | `push` | a | stack.push(ra); overflow ⇒ `STACK_OVERFLOW` | 2 |
| 28 | `pop` | a | ra = stack.pop(); empty ⇒ `STACK_UNDERFLOW` | 2 |
| 29 | `call` | i | calls.push(pc); pc = imm; depth cap ⇒ `STACK_OVERFLOW` | 2 |
| 30 | `ret` | | pc = calls.pop(); empty ⇒ `STACK_UNDERFLOW` | 2 |
| 31 | `alloc` | a b | ra = heap; heap += round8(rb); no room / rb < 0 ⇒ `OOM` | 5 |
| 32 | `sys` | i | syscall imm (below) | 10 |
| 33 | `trap` | i | raise imm | 1 |
| 34 | `seth` | i | handler = imm (−1 or out of range clears) | 1 |
| 35 | `not` | a b | ra = ¬rb | 1 |
| 36 | `neg` | a b | ra = −rb (wrapping) | 1 |
| 37 | `muli` | a b i | ra = rb × imm | 3 |

Any jump target outside `0..=len(code)` raises `BAD_JUMP`. Executing at
`pc = len(code)` halts.

## Syscalls (`sys n`)

| n | Name | Cap | In | Out |
|--:|---|---|---|---|
| 1 | `write` | Output | r1 ptr, r2 len | r0 = len; bytes appended to output |
| 2 | `ticks` | — | | r0 = gas used so far (charged before the syscall completes) |
| 3 | `rand` | — | | r0 = next seeded xorshift64\* value, non-negative |
| 4 | `host` | Host(r0) | r0 id, r1 r2 r3 args | r0 = result; unregistered ⇒ `BAD_SYSCALL`; Err ⇒ `HOST_ERROR` |
| 5 | `emit` | Emit | r1 ptr, r2 len | r0 = len; bytes pushed as one event |
| 6 | `read` | Input | r1 ptr, r2 max | r0 = bytes copied from host input |

Bad pointer/length ⇒ `MEM_OOB`. Missing capability ⇒ `CAP_DENIED`. Unknown
`n` ⇒ `BAD_SYSCALL`.

## Traps

| Code | Name |
|--:|---|
| 1 | `DIV_ZERO` |
| 2 | `MEM_OOB` |
| 3 | `STACK_OVERFLOW` (data or call stack) |
| 4 | `STACK_UNDERFLOW` |
| 5 | `ILLEGAL` (unknown opcode or register > 15) |
| 6 | `OOM` |
| 7 | `CAP_DENIED` |
| 8 | `BAD_SYSCALL` |
| 9 | `HOST_ERROR` |
| 10 | `BAD_JUMP` |
| ≥ 32 | user traps (`trap n`) by convention |

On a trap: if a handler is armed, `r0 = code`, `pc = handler`, the handler
is disarmed (re-arm with `seth`), execution continues. Otherwise status
becomes `Trapped(code)` and the VM will not run further.

Gas exhaustion is **not** a trap: it is `Status::OutOfGas`, decided by the
host, resumable with `resume(gas)`.

## Assembler syntax

```
; comment
.data name "string with \n \t \0 \\ \" \xNN escapes"   ; bytes; label = address
.data name 64                                          ; 64 zero bytes
.word name 12345                                       ; 8-byte little-endian
label:                                                 ; instruction index
  mnemonic operand, operand, operand
```

Operands: `rN`, decimal, `0x` hex, negative, `'c'` (with `\n \t \0 \\ \'`),
labels, and for `sys` the names `write ticks rand host emit read`. Immediates
must fit in signed 32 bits; build larger constants with `shl`/`or`.
