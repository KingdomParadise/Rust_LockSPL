use num_derive::FromPrimitive;
use solana_program::{decode_error::DecodeError, program_error::ProgramError};
use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, FromPrimitive, PartialEq)]
pub enum LockTokenError {
    #[error("Invalid Instruction")]
    InvalidInstruction
}

impl From<LockTokenError> for ProgramError {
    fn from(e: LockTokenError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

impl<T> DecodeError<T> for LockTokenError {
    fn type_of() -> &'static str {
        "LockTokenError"
    }
}
