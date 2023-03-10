use crate::stdlib::{borrow::Cow, collections::HashMap, prelude::*};
use crate::utils::CAIRO_PRIME;
use crate::{
    hint_processor::{
        builtin_hint_processor::hint_utils::{
            get_integer_from_var_name, get_relocatable_from_var_name,
        },
        hint_processor_definition::HintReference,
    },
    serde::deserialize_program::ApTracking,
    vm::{errors::hint_errors::HintError, vm_core::VirtualMachine},
};
use felt::Felt;
use lazy_static::lazy_static;
use num_bigint::BigUint;
use num_bigint::ToBigInt;
use num_traits::{Bounded, Num, One, Pow};
use sha2::{Digest, Sha256};

use crate::math_utils::sqrt;

#[derive(Debug, PartialEq)]
struct EcPoint<'a> {
    x: Cow<'a, Felt>,
    y: Cow<'a, Felt>,
}
impl EcPoint<'_> {
    fn from_var_name<'a>(
        name: &'a str,
        vm: &'a VirtualMachine,
        ids_data: &'a HashMap<String, HintReference>,
        ap_tracking: &'a ApTracking,
    ) -> Result<EcPoint<'a>, HintError> {
        // Get first addr of EcPoint struct
        let point_addr = get_relocatable_from_var_name(name, vm, ids_data, ap_tracking)?;
        Ok(EcPoint {
            x: vm
                .get_integer(point_addr)
                .map_err(|_| HintError::IdentifierHasNoMember(name.to_string(), "x".to_string()))?,
            y: vm
                .get_integer((point_addr + 1)?)
                .map_err(|_| HintError::IdentifierHasNoMember(name.to_string(), "x".to_string()))?,
        })
    }
}

// Implements hint:
// from starkware.crypto.signature.signature import ALPHA, BETA, FIELD_PRIME
// from starkware.python.math_utils import random_ec_point
// from starkware.python.utils import to_bytes

// # Define a seed for random_ec_point that's dependent on all the input, so that:
// #   (1) The added point s is deterministic.
// #   (2) It's hard to choose inputs for which the builtin will fail.
// seed = b"".join(map(to_bytes, [ids.p.x, ids.p.y, ids.m, ids.q.x, ids.q.y]))
// ids.s.x, ids.s.y = random_ec_point(FIELD_PRIME, ALPHA, BETA, seed)

pub fn random_ec_point_hint(
    vm: &mut VirtualMachine,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), HintError> {
    let p = EcPoint::from_var_name("p", vm, ids_data, ap_tracking)?;
    let q = EcPoint::from_var_name("q", vm, ids_data, ap_tracking)?;
    let m = get_integer_from_var_name("m", vm, ids_data, ap_tracking)?;
    let bytes: Vec<u8> = [p.x, p.y, m, q.x, q.y]
        .iter()
        .flat_map(|x| to_padded_bytes(&x))
        .collect();
    let (x, y) = random_ec_point(bytes)?;
    let s_addr = get_relocatable_from_var_name("s", vm, ids_data, ap_tracking)?;
    vm.insert_value(s_addr, x)?;
    vm.insert_value((s_addr + 1)?, y)?;
    Ok(())
}

// Returns the Felt as a vec of bytes of len 32, pads left with zeros
fn to_padded_bytes(n: &Felt) -> Vec<u8> {
    let felt_to_bytes = n.to_bytes_be();
    let mut bytes: Vec<u8> = vec![0; 32 - felt_to_bytes.len()];
    bytes.extend(felt_to_bytes);
    bytes
}

// Returns a random non-zero point on the elliptic curve
//   y^2 = x^3 + alpha * x + beta (mod field_prime).
// The point is created deterministically from the seed.
fn random_ec_point(seed_bytes: Vec<u8>) -> Result<(Felt, Felt), HintError> {
    // Hash initial seed
    let mut hasher = Sha256::new();
    hasher.update(seed_bytes);
    let seed = hasher.finalize_reset().to_vec();
    for i in 0..100 {
        // Calculate x
        let i_bytes = (i as u8).to_le_bytes();
        let mut input = seed[1..].to_vec();
        input.extend(i_bytes);
        input.extend(vec![0; 10 - i_bytes.len()]);
        hasher.update(input);
        let x = BigUint::from_bytes_be(&hasher.finalize_reset());
        // Calculate y
        let y_coef = (-1).pow(seed[0] & 1);
        let y = recover_y(&x);
        if let Some(y) = y {
            // Conversion from BigUint to BigInt doesnt fail
            return Ok((Felt::from(x), Felt::from(y.to_bigint().unwrap() * y_coef)));
        }
    }
    Err(HintError::RandomEcPointNotOnCurve)
}
const ALPHA: u32 = 1;
lazy_static! {
    static ref BETA: BigUint = BigUint::from_str_radix(
        "3141592653589793238462643383279502884197169399375105820974944592307816406665",
        10
    )
    .unwrap();
}

// Recovers the corresponding y coordinate on the elliptic curve
//     y^2 = x^3 + alpha * x + beta (mod field_prime)
//     of a given x coordinate.
// Returns None if x is not the x coordinate of a point in the curve
fn recover_y(x: &BigUint) -> Option<BigUint> {
    let y_squared: BigUint = x.modpow(&BigUint::from(3_u32), &*CAIRO_PRIME) + ALPHA * x + &*BETA;
    if is_quad_residue(&y_squared) {
        Some(sqrt(&Felt::from(y_squared)).to_biguint())
    } else {
        None
    }
}

// Implementation adapted from sympy implementation
// Conditions:
// + prime is ommited as it will be CAIRO_PRIME
// + a >= 0 < prime (other cases ommited)
fn is_quad_residue(a: &BigUint) -> bool {
    if a < &BigUint::from(2_u8) {
        return true;
    };
    a.modpow(&(Felt::max_value().to_biguint() / 2_u32), &*CAIRO_PRIME) == BigUint::one()
}
