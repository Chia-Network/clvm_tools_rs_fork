use clvm_rs::node::Node;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::io::{Error, ErrorKind, SeekFrom};
use std::os::raw::c_char;
use std::rc::Rc;

use clvm_rs::allocator::{Allocator, NodePtr, SExp};
use clvm_rs::cost::Cost;
use clvm_rs::f_table::{f_lookup_for_hashmap, FLookup};
use clvm_rs::more_ops::op_unknown;
use clvm_rs::operator_handler::OperatorHandler;
use clvm_rs::reduction::{EvalErr, Reduction, Response};

use clvm_rs::run_program::{run_program, PreEval};

use crate::classic::clvm::sexp::CastableType::String;
use crate::classic::clvm_tools::sha256tree::sha256tree;
use crate::classic::clvm_tools::stages::stage_0::{
    DefaultProgramRunner, OpRouter, RunProgramOption, TRunProgram,
};
use crate::clvm_api::ClvmResponse;
use clvm_rs::chia_dialect::chia_dialect;

const MAX_SINGLE_BYTE: u8 = 0x7f;
const CONS_BOX_MARKER: u8 = 0xff;

fn bad_encoding() -> std::io::Error {
    Error::new(ErrorKind::InvalidInput, "bad encoding")
}

fn internal_error() -> std::io::Error {
    Error::new(ErrorKind::InvalidInput, "internal error")
}

fn encode_size(f: &mut dyn Write, size: u64) -> std::io::Result<()> {
    if size < 0x40 {
        f.write_all(&[(0x80 | size) as u8])?;
    } else if size < 0x2000 {
        f.write_all(&[(0xc0 | (size >> 8)) as u8, ((size) & 0xff) as u8])?;
    } else if size < 0x10_0000 {
        f.write_all(&[
            (0xe0 | (size >> 16)) as u8,
            ((size >> 8) & 0xff) as u8,
            ((size) & 0xff) as u8,
        ])?;
    } else if size < 0x800_0000 {
        f.write_all(&[
            (0xf0 | (size >> 24)) as u8,
            ((size >> 16) & 0xff) as u8,
            ((size >> 8) & 0xff) as u8,
            ((size) & 0xff) as u8,
        ])?;
    } else if size < 0x4_0000_0000 {
        f.write_all(&[
            (0xf8 | (size >> 32)) as u8,
            ((size >> 24) & 0xff) as u8,
            ((size >> 16) & 0xff) as u8,
            ((size >> 8) & 0xff) as u8,
            ((size) & 0xff) as u8,
        ])?;
    } else {
        return Err(Error::new(ErrorKind::InvalidData, "atom too big"));
    }
    Ok(())
}

pub fn prepare_response_for_flutter(node: NodePtr) -> std::io::Result<Vec<ClvmResponse>> {
    let mut values: Vec<NodePtr> = vec![node];
    let mut values_response: Vec<ClvmResponse> = vec![];

    let a = Allocator::new();
    while !values.is_empty() {
        let v = values.pop().unwrap();
        let n = a.sexp(v);
        match n {
            SExp::Atom(atom_ptr) => {
                let atom = a.buf(&atom_ptr);
                let size = atom.len();
                if size == 0 {
                    // f.write_all(&[0x80_u8])?;
                    let value = ClvmResponse {
                        value_type: std::string::String::from("empty"),
                        value: vec![0x80_u8],
                        encoded: std::string::String::from("u8"),
                        value_len: 0,
                    };
                    values_response.push(value);
                } else {
                    let atom0 = atom[0];
                    if size == 1 && (atom0 <= MAX_SINGLE_BYTE) {
                        let value = ClvmResponse {
                            value_type: std::string::String::from("byte"),
                            value: vec![atom0],
                            encoded: std::string::String::from("u8"),
                            value_len: 1,
                        };
                        values_response.push(value);
                    } else {
                        let mut buffer = Cursor::new(Vec::new());
                        encode_size(&mut buffer, size as u64)?;
                        buffer.write_all(atom)?;
                        let value = ClvmResponse {
                            value_type: std::string::String::from("buffer"),
                            value: buffer.into_inner(),
                            encoded: std::string::String::from("u64"),
                            value_len: size as i32,
                        };
                        values_response.push(value);
                    }
                }
            }
            SExp::Pair(left, right) => {
                // f.write_all(&[CONS_BOX_MARKER as u8])?;

                let value = ClvmResponse {
                    value_type: std::string::String::from("CONS_BOX_MARKER"),
                    value: vec![CONS_BOX_MARKER as u8],
                    encoded: std::string::String::from("u8"),
                    value_len: 1,
                };
                values_response.push(value);
                values.push(right);
                values.push(left);
            }
        }
    }
    Ok(values_response)
}

pub fn node_to_stream(node: &Node, f: &mut dyn Write) -> std::io::Result<()> {
    let mut values: Vec<NodePtr> = vec![node.node];
    let a = node.allocator;
    while !values.is_empty() {
        let v = values.pop().unwrap();
        let n = a.sexp(v);
        match n {
            SExp::Atom(atom_ptr) => {
                let atom = a.buf(&atom_ptr);
                let size = atom.len();
                if size == 0 {
                    f.write_all(&[0x80_u8])?;
                } else {
                    let atom0 = atom[0];
                    if size == 1 && (atom0 <= MAX_SINGLE_BYTE) {
                        f.write_all(&[atom0])?;
                    } else {
                        encode_size(f, size as u64)?;
                        f.write_all(atom)?;
                    }
                }
            }
            SExp::Pair(left, right) => {
                f.write_all(&[CONS_BOX_MARKER as u8])?;
                values.push(right);
                values.push(left);
            }
        }
    }
    Ok(())
}

fn decode_size(f: &mut dyn Read, initial_b: u8) -> std::io::Result<u64> {
    // this function decodes the length prefix for an atom. Atoms whose value
    // fit in 7 bits don't have a length-prefix, so those should never be passed
    // to this function.
    debug_assert!((initial_b & 0x80) != 0);
    if (initial_b & 0x80) == 0 {
        return Err(internal_error());
    }

    let mut bit_count = 0;
    let mut bit_mask: u8 = 0x80;
    let mut b = initial_b;
    while b & bit_mask != 0 {
        bit_count += 1;
        b &= 0xff ^ bit_mask;
        bit_mask >>= 1;
    }
    let mut size_blob: Vec<u8> = Vec::new();
    size_blob.resize(bit_count, 0);
    size_blob[0] = b;
    if bit_count > 1 {
        let remaining_buffer = &mut size_blob[1..];
        f.read_exact(remaining_buffer)?;
    }
    // need to convert size_blob to an int
    let mut v: u64 = 0;
    if size_blob.len() > 6 {
        return Err(bad_encoding());
    }
    for b in &size_blob {
        v <<= 8;
        v += *b as u64;
    }
    if v >= 0x400000000 {
        return Err(bad_encoding());
    }
    Ok(v)
}

enum ParseOp {
    SExp,
    Cons,
}

/* impl std::convert::From<EvalErr> for std::io::Error {
    fn from(v: EvalErr) -> Self {
        Self::new(ErrorKind::Other, v.1)
    }
} */

pub fn node_from_stream(
    allocator: &mut Allocator,
    f: &mut Cursor<&[u8]>,
) -> std::io::Result<NodePtr> {
    let mut values: Vec<NodePtr> = Vec::new();
    let mut ops = vec![ParseOp::SExp];

    let mut b = [0; 1];
    loop {
        let op = ops.pop();
        if op.is_none() {
            break;
        }
        match op.unwrap() {
            ParseOp::SExp => {
                f.read_exact(&mut b)?;
                if b[0] == CONS_BOX_MARKER {
                    ops.push(ParseOp::Cons);
                    ops.push(ParseOp::SExp);
                    ops.push(ParseOp::SExp);
                } else if b[0] == 0x01 {
                    values.push(allocator.one());
                } else if b[0] == 0x80 {
                    values.push(allocator.null());
                } else if b[0] <= MAX_SINGLE_BYTE {
                    values.push(allocator.new_atom(&b)?);
                } else {
                    let blob_size = decode_size(f, b[0])?;
                    if (f.get_ref().len() as u64) < blob_size {
                        return Err(bad_encoding());
                    }
                    let mut blob: Vec<u8> = vec![0; blob_size as usize];
                    f.read_exact(&mut blob)?;
                    values.push(allocator.new_atom(&blob)?);
                }
            }
            ParseOp::Cons => {
                // cons
                let v2 = values.pop();
                let v1 = values.pop();
                values.push(allocator.new_pair(v1.unwrap(), v2.unwrap())?);
            }
        }
    }
    Ok(values.pop().unwrap())
}

pub fn node_from_bytes(allocator: &mut Allocator, b: &[u8]) -> std::io::Result<NodePtr> {
    let mut buffer = Cursor::new(b);
    node_from_stream(allocator, &mut buffer)
}

pub fn node_to_bytes(node: &Node) -> std::io::Result<Vec<u8>> {
    let mut buffer = Cursor::new(Vec::new());

    node_to_stream(node, &mut buffer)?;
    let vec = buffer.into_inner();
    Ok(vec)
}

pub fn serialized_length_from_bytes(b: &[u8]) -> std::io::Result<u64> {
    let mut f = Cursor::new(b);
    let mut ops = vec![ParseOp::SExp];
    let mut b = [0; 1];
    loop {
        let op = ops.pop();
        if op.is_none() {
            break;
        }
        match op.unwrap() {
            ParseOp::SExp => {
                f.read_exact(&mut b)?;
                if b[0] == CONS_BOX_MARKER {
                    // since all we're doing is to determing the length of the
                    // serialized buffer, we don't need to do anything about
                    // "cons". So we skip pushing it to lower the pressure on
                    // the op stack
                    //ops.push(ParseOp::Cons);
                    ops.push(ParseOp::SExp);
                    ops.push(ParseOp::SExp);
                } else if b[0] == 0x80 || b[0] <= MAX_SINGLE_BYTE {
                    // This one byte we just read was the whole atom.
                    // or the
                    // special case of NIL
                } else {
                    let blob_size = decode_size(&mut f, b[0])?;
                    f.seek(SeekFrom::Current(blob_size as i64))?;
                    if (f.get_ref().len() as u64) < f.position() {
                        return Err(bad_encoding());
                    }
                }
            }
            ParseOp::Cons => {
                // cons. No need to construct any structure here. Just keep
                // going
            }
        }
    }
    Ok(f.position())
}
