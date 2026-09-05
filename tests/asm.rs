//! Assembler / disassembler: round trips, labels, data directives, errors.

use halyard::{Instr, Op, Program, asm};

#[test]
fn round_trip_through_disassembler() {
    let src = ".data msg \"Hi\\n\"\n.word n 42\nstart:\n movi r1, msg\n movi r2, 3\n sys write\n ld r3, r1, 8\n jlt r3, r2, start\n trap 40\n halt";
    let p = asm::assemble(src).unwrap();
    let dis = asm::disassemble(&p);
    // Strip pc prefixes and data comments, re-assemble, compare code.
    let again: String = dis
        .lines()
        .filter(|l| !l.starts_with(';'))
        .map(|l| l[6..].to_string() + "\n")
        .collect();
    let p2 = asm::assemble(&again).unwrap();
    assert_eq!(p.code, p2.code);
    assert!(dis.contains("sys write"));
    assert!(dis.contains("jlt r3, r2, 0"));
    assert!(dis.contains("48 69 0a"), "data dump: {dis}");
}

#[test]
fn data_directives_and_labels() {
    let p = asm::assemble(
        ".data a \"AB\"\n.data pad 6\n.word w -1\n.data c 'z'\nmovi r1, w\nmovi r2, c",
    )
    .unwrap();
    // 'z' is the integer 122, so `.data c 'z'` reserves 122 zero bytes.
    assert_eq!(p.data.len(), 2 + 6 + 8 + 122);
    assert_eq!(&p.data[..2], b"AB");
    assert_eq!(&p.data[8..16], &(-1i64).to_le_bytes());
    assert_eq!(p.code[0].imm, 8);
    assert_eq!(p.code[1].imm, 16);
}

#[test]
fn immediates_and_chars() {
    let p = asm::assemble(
        "movi r1, 0x1f\n movi r2, -0x10\n movi r3, 'a'\n movi r4, '\\n'\n addi r5, r5, 2147483647",
    )
    .unwrap();
    assert_eq!(
        p.code.iter().map(|i| i.imm).collect::<Vec<_>>(),
        vec![31, -16, 97, 10, i32::MAX]
    );
}

#[test]
fn errors_are_reported_with_line_numbers() {
    let e = asm::assemble("nop\n bogus r1").unwrap_err();
    assert_eq!(e.line, 2);
    assert!(e.msg.contains("unknown instruction"));
    let e = asm::assemble("movi r1").unwrap_err();
    assert!(e.msg.contains("takes 2 operand"));
    let e = asm::assemble("movi 5, r1").unwrap_err();
    assert!(e.msg.contains("must be a register"));
    let e = asm::assemble("jmp nowhere").unwrap_err();
    assert!(e.msg.contains("undefined label nowhere"));
    let e = asm::assemble("x:\nx:\n nop").unwrap_err();
    assert!(e.msg.contains("duplicate label"));
    let e = asm::assemble("movi r1, 4294967296").unwrap_err();
    assert!(e.msg.contains("32 bits"));
    let e = asm::assemble("movi r16, 1").unwrap_err();
    assert!(e.msg.contains("register"));
    let e = asm::assemble("mov r1, 5").unwrap_err();
    assert!(e.msg.contains("register"));
}

#[test]
fn comments_and_quotes_do_not_confuse_each_other() {
    let p =
        asm::assemble(".data s \"a;b\" ; real comment\n movi r1, ';' ; another\n halt").unwrap();
    assert_eq!(p.data, b"a;b");
    assert_eq!(p.code[0].imm, b';' as i32);
}

#[test]
fn binary_encoding_is_stable() {
    let i = Instr::new(Op::Jlt, 3, 2, 0, -7);
    assert_eq!(i.to_bytes(), [19, 3, 2, 0, 0xf9, 0xff, 0xff, 0xff]);
    assert_eq!(Instr::from_bytes(i.to_bytes()), i);
    let p = Program {
        code: vec![i],
        data: vec![1, 2, 3],
    };
    let b = p.to_bytes();
    assert_eq!(&b[..4], b"HLYD");
    assert_eq!(b.len(), 16 + 8 + 3);
    assert_eq!(Program::from_bytes(&b).unwrap(), p);
}
