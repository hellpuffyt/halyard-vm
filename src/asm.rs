//! Assembler and disassembler for Halyard's text format.
//!
//! ```text
//! ; comments start with ';'
//! .data greeting "Hello\n"     ; bytes in the data segment; label = address
//! .data buf 64                 ; 64 zero bytes
//! .word counter 42             ; one 8-byte little-endian integer
//!
//! main:                        ; code label = instruction index
//!   movi r1, greeting
//!   movi r2, 6
//!   sys write                  ; syscall by name or number
//!   jmp done
//! done:
//!   halt
//! ```
//!
//! Operands are registers `r0`–`r15`, decimal/hex (`0x1f`) immediates
//! (negative allowed), labels, or `'c'` character literals.

use std::collections::HashMap;

use crate::{Instr, Op, Program, sys};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsmError {
    pub line: usize,
    pub msg: String,
}

impl std::fmt::Display for AsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

fn err<T>(line: usize, msg: impl Into<String>) -> Result<T, AsmError> {
    Err(AsmError {
        line,
        msg: msg.into(),
    })
}

fn unescape(s: &str, line: usize) -> Result<Vec<u8>, AsmError> {
    let mut out = Vec::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match it.next() {
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('0') => out.push(0),
            Some('\\') => out.push(b'\\'),
            Some('"') => out.push(b'"'),
            Some('x') => {
                let hi = it.next().and_then(|c| c.to_digit(16));
                let lo = it.next().and_then(|c| c.to_digit(16));
                match (hi, lo) {
                    (Some(h), Some(l)) => out.push((h * 16 + l) as u8),
                    _ => return err(line, "bad \\x escape"),
                }
            }
            other => return err(line, format!("bad escape \\{}", other.unwrap_or(' '))),
        }
    }
    Ok(out)
}

/// Splits `a, b, c` respecting quotes; trims each part.
fn split_operands(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            None if c == ',' => {
                parts.push(cur.trim().to_string());
                cur.clear();
            }
            None => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}

fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ';' => return &line[..i],
            None => {}
        }
    }
    line
}

enum Pending {
    Instr(Instr, Vec<(usize, String)>),
}

/// Assembles source text into a [`Program`].
pub fn assemble(src: &str) -> Result<Program, AsmError> {
    let mut labels: HashMap<String, i64> = HashMap::new();
    let mut data = Vec::new();
    let mut code: Vec<(usize, Pending)> = Vec::new();

    for (idx, raw) in src.lines().enumerate() {
        let ln = idx + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix(".data")
            .or_else(|| line.strip_prefix(".word"))
        {
            let is_word = line.starts_with(".word");
            let rest = rest.trim();
            let (name, value) = rest
                .split_once(char::is_whitespace)
                .ok_or_else(|| AsmError {
                    line: ln,
                    msg: "expected `.data name value`".into(),
                })?;
            let value = value.trim();
            if labels.insert(name.to_string(), data.len() as i64).is_some() {
                return err(ln, format!("duplicate label {name}"));
            }
            if is_word {
                let v = parse_int(value).ok_or_else(|| AsmError {
                    line: ln,
                    msg: format!("bad integer {value}"),
                })?;
                data.extend_from_slice(&v.to_le_bytes());
            } else if let Some(s) = value.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                data.extend_from_slice(&unescape(s, ln)?);
            } else if let Some(n) = parse_int(value) {
                if n < 0 {
                    return err(ln, "negative reservation");
                }
                data.resize(data.len() + n as usize, 0);
            } else {
                return err(ln, format!("bad .data value {value}"));
            }
            continue;
        }
        if let Some(name) = line.strip_suffix(':') {
            let name = name.trim();
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return err(ln, format!("bad label {name:?}"));
            }
            if labels.insert(name.to_string(), code.len() as i64).is_some() {
                return err(ln, format!("duplicate label {name}"));
            }
            continue;
        }
        let (mn, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let op = Op::from_mnemonic(&mn.to_ascii_lowercase()).ok_or_else(|| AsmError {
            line: ln,
            msg: format!("unknown instruction {mn}"),
        })?;
        let ops = split_operands(rest);
        let shape = op.shape();
        if ops.len() != shape.len() {
            return err(
                ln,
                format!("{mn} takes {} operand(s), got {}", shape.len(), ops.len()),
            );
        }
        let mut ins = Instr {
            op: op as u8,
            ..Default::default()
        };
        let mut fixups = Vec::new();
        for (k, (want, text)) in shape.chars().zip(&ops).enumerate() {
            match want {
                'a' | 'b' | 'c' => {
                    let r = parse_reg(text).ok_or_else(|| AsmError {
                        line: ln,
                        msg: format!("operand {} of {mn} must be a register, got {text}", k + 1),
                    })?;
                    match want {
                        'a' => ins.a = r,
                        'b' => ins.b = r,
                        _ => ins.c = r,
                    }
                }
                _ => {
                    if op == Op::Sys
                        && let Some(n) = sys::from_name(text)
                    {
                        ins.imm = n;
                    } else if let Some(v) = parse_int(text) {
                        ins.imm = i32::try_from(v).map_err(|_| AsmError {
                            line: ln,
                            msg: format!("immediate {v} does not fit in 32 bits"),
                        })?;
                    } else if parse_reg(text).is_some() {
                        return err(
                            ln,
                            format!("{mn} expects an immediate or label, got register {text}"),
                        );
                    } else {
                        fixups.push((ln, text.clone()));
                    }
                }
            }
        }
        code.push((ln, Pending::Instr(ins, fixups)));
    }

    let mut out = Vec::with_capacity(code.len());
    for (_, Pending::Instr(mut ins, fixups)) in code {
        for (ln, name) in fixups {
            let v = *labels.get(&name).ok_or_else(|| AsmError {
                line: ln,
                msg: format!("undefined label {name}"),
            })?;
            ins.imm = i32::try_from(v).map_err(|_| AsmError {
                line: ln,
                msg: "label out of range".into(),
            })?;
        }
        out.push(ins);
    }
    Ok(Program { code: out, data })
}

fn parse_reg(s: &str) -> Option<u8> {
    let n: u8 = s.strip_prefix('r')?.parse().ok()?;
    (n < 16).then_some(n)
}

fn parse_int(s: &str) -> Option<i64> {
    if let Some(c) = s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        let mut chars = c.chars();
        let ch = match chars.next()? {
            '\\' => match chars.next()? {
                'n' => '\n',
                't' => '\t',
                '0' => '\0',
                '\\' => '\\',
                '\'' => '\'',
                _ => return None,
            },
            ch => ch,
        };
        return chars.next().is_none().then_some(ch as i64);
    }
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let v = if let Some(h) = s.strip_prefix("0x") {
        i64::from_str_radix(h, 16).ok()?
    } else {
        s.parse::<i64>().ok()?
    };
    Some(if neg { -v } else { v })
}

/// Renders one instruction the way the assembler would accept it.
pub fn format_instr(i: Instr) -> String {
    let Some(op) = Op::from_u8(i.op) else {
        return format!("??? 0x{:02x}", i.op);
    };
    let mut parts = Vec::new();
    for ch in op.shape().chars() {
        parts.push(match ch {
            'a' => format!("r{}", i.a),
            'b' => format!("r{}", i.b),
            'c' => format!("r{}", i.c),
            _ => {
                if op == Op::Sys {
                    sys::name(i.imm).map_or_else(|| i.imm.to_string(), str::to_string)
                } else {
                    i.imm.to_string()
                }
            }
        });
    }
    if parts.is_empty() {
        op.mnemonic().to_string()
    } else {
        format!("{} {}", op.mnemonic(), parts.join(", "))
    }
}

/// Disassembles a whole program: numbered code lines, then a data dump.
pub fn disassemble(p: &Program) -> String {
    let mut s = String::new();
    for (pc, i) in p.code.iter().enumerate() {
        s.push_str(&format!("{pc:04}  {}\n", format_instr(*i)));
    }
    if !p.data.is_empty() {
        s.push_str(&format!("; data ({} bytes)\n", p.data.len()));
        for (off, chunk) in p.data.chunks(16).enumerate() {
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
            s.push_str(&format!(
                "; {:04x}  {:<48} {txt}\n",
                off * 16,
                hex.join(" ")
            ));
        }
    }
    s
}
