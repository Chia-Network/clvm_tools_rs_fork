use crate::classic::clvm::__type_compatibility__::{Bytes, BytesFromType};
use crate::classic::clvm::sexp::{to_sexp_type, CastableType};
use crate::clvm_api::{ArgBytesType, ClvmArg};
use crate::util::number_from_u8;
use bls12_381::G1Affine;
use clvm_rs::allocator::{Allocator, NodePtr};
use clvm_rs::reduction::{EvalErr, Reduction};
 
use std::borrow::Borrow;
use std::convert::TryInto;
use std::rc::Rc;

// Convert Flutter arguments to to_sexp_type
pub fn to_clvm_object(allocator: &mut Allocator, argument: &ClvmArg) -> Result<NodePtr, EvalErr> {
    let result = convert_to_casteable_type(argument).unwrap();
    return to_sexp_type(allocator, result);
}
pub fn convert_to_casteable_type(argument: &ClvmArg) -> Result<CastableType, EvalErr> {
    return match argument.value_type {
        ArgBytesType::Hex => to_hex_type(argument),
        ArgBytesType::Bytes => to_bytes_type(argument),
        ArgBytesType::String => to_string_type(argument),
        ArgBytesType::Number => to_number_type(argument),
        ArgBytesType::G1Affine => to_g1_affine_type(argument),
        ArgBytesType::ListOf => to_list_of_type(argument),
        ArgBytesType::TupleOf => to_tuple_of_type(argument),
    };
}

pub fn to_hex_type(hex_text: &ClvmArg) -> Result<CastableType, EvalErr> {
   //error!("to_hex_type {:?}", hex_text.value);
    return to_bytes_type(hex_text);
}

pub fn to_string_type(str: &ClvmArg) -> Result<CastableType, EvalErr> {
   //error!("to_string_type {:?}", str.value);
    return Ok(CastableType::String(
        String::from_utf8(str.clone().value).unwrap(),
    ));
}

pub fn to_number_type(number: &ClvmArg) -> Result<CastableType, EvalErr> {
   //error!("to_number_type {:?}", number.value);
    let number_value = number_from_u8(number.value.borrow());
   //error!("number_value {:?}", number_value);

    return Ok(CastableType::Number(number_value));
}

pub fn to_g1_affine_type(number: &ClvmArg) -> Result<CastableType, EvalErr> {
   //error!("to_g1_affine_type {:?}", number.value);
    let bytes_array: [u8; 48] = vector_to_fixed_array(number.clone().value);
    let g1_affline = G1Affine::from_compressed(&bytes_array).unwrap();

    return Ok(CastableType::G1Affine(g1_affline));
}

fn vector_to_fixed_array<T>(v: Vec<T>) -> [T; 48]
where
    T: Copy,
{
    let slice = v.as_slice();
    let array: [T; 48] = match slice.try_into() {
        Ok(ba) => ba,
        Err(_) => panic!("Expected a Vec of length {} but it was {}", 48, v.len()),
    };
    array
}
pub fn to_list_of_type(list: &ClvmArg) -> Result<CastableType, EvalErr> {
   //error!("to_list_of_type {}", list.children.len());
    let mut stack: Vec<Rc<CastableType>> = Vec::new();
    let temp_alocattor = Allocator::new();
    //stack.push(Rc::new(CastableType::CLVMObject(temp_alocattor.null())));
    let len = list.children.len();
    for i in 0..len {
       //error!("i {}", i);
        let item = list.children[i].borrow();
        let clvm_object = convert_to_casteable_type(item).unwrap();
        stack.push(Rc::new(clvm_object));
    }
    return Ok(CastableType::ListOf(stack.len(), stack));
}

pub fn to_tuple_of_type(list: &ClvmArg) -> Result<CastableType, EvalErr> {
    let temp_alocattor = Allocator::new();
    let mut rigth: CastableType = CastableType::CLVMObject(temp_alocattor.null());
    let mut left: CastableType = CastableType::CLVMObject(temp_alocattor.null());

    for i in 0..1 {
        let item = list.children[i].borrow();
        let clvm_object = convert_to_casteable_type(item).unwrap();

        if i == 0 {
            rigth = clvm_object;
        } else {
            left = clvm_object;
        }
    }

    return Ok(CastableType::TupleOf(Rc::new(rigth), Rc::new(left)));
}

pub fn to_bytes_type(raw: &ClvmArg) -> Result<CastableType, EvalErr> {
    return Ok(CastableType::Bytes(Bytes::new(Some(BytesFromType::Raw(
        raw.value.clone(),
    )))));
}
