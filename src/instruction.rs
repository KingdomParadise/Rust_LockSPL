use crate::error::LockTokenError;

use solana_program::{
    instruction::{AccountMeta, Instruction},
    msg,
    program_error::ProgramError,
    pubkey::Pubkey
};

use std::convert::TryInto;
use std::mem::size_of;

#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub struct Schedule {
    pub release_time: u64,
    pub amount: u64,
}

pub const SCHEDULE_SIZE: usize = 16;

#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub enum LockTokenInstruction {
    /* Inits a new lock schedule.
    *  A lock schedule consists of a LockScheduleHeader and array of LockSchedule s.
    *  The header consists of destination address, token mint address and initialized flag.
    *  LockTokenInstruction::Init instruction creates a program account from the seeds array which has data size to fit the number of schedule data.
    *
    *  - Accounts
    *  0. `[]` The system program account
    *  1. `[]` The sysvar Rent account
    *  2. `[signer]` The fee payer account
    *  3. `[]` The locking account
    */
    Init {
        seeds: [u8; 32],
        number_of_schedules: u32,
    },

    /* Creates a new lock schedule.
    *  Actually, fills data into account which is created by Init instruction.
    *  LockTokenInstruction::Init instruction creates a program account from the seeds array which has data size to fit the number of schedule data.
    *  The locking token account is needed to be derived from the locking account and token mint address by associated token account porogram.
    *  The source token account owner need to pay transaction fee for both solana network and company.
    *
    *  - Accounts
    *  0. `[]` The spl token program account
    *  1. `[]` The locking account
    *  2. `[]` The locking token account
    *  3. `[signer]` The source token account owner
    *  4. `[]` The source token account
    *  5. `[]` The token state account
    *  6. `[]` The company wallet account
    */
    Create {
        seeds: [u8; 32],
        mint_address: Pubkey,
        destination_token_address: Pubkey,
        schedules: Vec<Schedule>,
    },

    Unlock { seeds: [u8; 32] },

    TransferLocks { seeds: [u8; 32] },

    ExtendLockDuration {
        seeds: [u8; 32],
        index: u32,
        release_time: u64,
    },

//////////////////////////////////////
    PauseContract {
        is_pause: bool,
    },

    SetFeeParams {
        price_estimator: Pubkey,
        usd_token_address: Pubkey,
        fees_in_usd: u64,
        company_wallet: Pubkey,
    },

    SetFeesInUSD {
        fees_in_usd: u64,
    },

    SetCompanyWallet {
        company_wallet: Pubkey,
    },

    SetFreeToken {
        mint_address: Pubkey,
        is_free: bool,
    },

    TransferOwnership {},
}

impl LockTokenInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        use LockTokenError::InvalidInstruction;
        let (&tag, rest) = input.split_first().ok_or(InvalidInstruction)?;
        Ok(match tag {
            0 => {
                let seeds: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let number_of_schedules = rest
                    .get(32..36)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u32::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::Init {
                    seeds,
                    number_of_schedules,
                }
            }
            1 => {
                let seeds: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let mint_address = rest
                    .get(32..64)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                let destination_token_address = rest
                    .get(64..96)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                let number_of_schedules = rest[96..].len() / SCHEDULE_SIZE;
                let mut schedules: Vec<Schedule> = Vec::with_capacity(number_of_schedules);
                let mut offset = 96;
                for _ in 0..number_of_schedules {
                    let release_time = rest
                        .get(offset..offset + 8)
                        .and_then(|slice| slice.try_into().ok())
                        .map(u64::from_le_bytes)
                        .ok_or(InvalidInstruction)?;
                    let amount = rest
                        .get(offset + 8..offset + 16)
                        .and_then(|slice| slice.try_into().ok())
                        .map(u64::from_le_bytes)
                        .ok_or(InvalidInstruction)?;
                    offset += SCHEDULE_SIZE;
                    schedules.push(Schedule {
                        release_time,
                        amount,
                    })
                }
                Self::Create {
                    seeds,
                    mint_address,
                    destination_token_address,
                    schedules,
                }
            }
            2 | 3 => {
                let seeds: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                match tag {
                    2 => Self::Unlock { seeds },
                    _ => Self::TransferLocks { seeds },
                }
            }
            4 => {
                let seeds: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let index: u32 = rest
                    .get(32..36)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u32::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let release_time: u64 = rest
                    .get(36..44)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::ExtendLockDuration {
                    seeds,
                    index,
                    release_time,
                }
            }
            5 => {
                let is_pause_u8: u8 = rest
                    .get(..1)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u8::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let is_pause: bool = is_pause_u8 == 1;
                Self::PauseContract {
                    is_pause,
                }
            }
            6 => {
                let price_estimator = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                let usd_token_address = rest
                    .get(32..64)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                let fees_in_usd = rest
                    .get(64..72)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let company_wallet = rest
                    .get(72..104)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                Self::SetFeeParams {
                    price_estimator,
                    usd_token_address,
                    fees_in_usd,
                    company_wallet,
                }
            }
            7 => {
                let fees_in_usd = rest
                    .get(..8)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::SetFeesInUSD {
                    fees_in_usd,
                }
            }
            8 => {
                let company_wallet = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                Self::SetCompanyWallet {
                    company_wallet,
                }
            }
            9 => {
                let mint_address = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                let is_free_u8: u8 = rest
                    .get(32..33)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u8::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let is_free: bool = is_free_u8 == 1;
                Self::SetFreeToken {
                    mint_address,
                    is_free,
                }
            }
            10 => {
                Self::TransferOwnership {}
            }
            _ => {
                msg!("Unsupported tag");
                return Err(InvalidInstruction.into());
            }
        })
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        match self {
            &Self::Init {
                seeds,
                number_of_schedules,
            } => {
                buf.push(0);
                buf.extend_from_slice(&seeds);
                buf.extend_from_slice(&number_of_schedules.to_le_bytes())
            }
            Self::Create {
                seeds,
                mint_address,
                destination_token_address,
                schedules,
            } => {
                buf.push(1);
                buf.extend_from_slice(seeds);
                buf.extend_from_slice(&mint_address.to_bytes());
                buf.extend_from_slice(&destination_token_address.to_bytes());
                for s in schedules.iter() {
                    buf.extend_from_slice(&s.release_time.to_le_bytes());
                    buf.extend_from_slice(&s.amount.to_le_bytes());
                }
            }
            &Self::Unlock { seeds } => {
                buf.push(2);
                buf.extend_from_slice(&seeds);
            }
            &Self::TransferLocks { seeds } => {
                buf.push(3);
                buf.extend_from_slice(&seeds);
            }
            &Self::ExtendLockDuration {
                seeds,
                index,
                release_time,
            } => {
                buf.push(4);
                buf.extend_from_slice(&seeds);
                buf.extend_from_slice(&index.to_le_bytes());
                buf.extend_from_slice(&release_time.to_le_bytes());
            }
            &Self::PauseContract {
                is_pause,
            } => {
                buf.push(5);
                buf.extend_from_slice(&(is_pause as u8).to_le_bytes());
            }
            &Self::SetFeeParams {
                price_estimator,
                usd_token_address,
                fees_in_usd,
                company_wallet,
            } => {
                buf.push(6);
                buf.extend_from_slice(&price_estimator.to_bytes());
                buf.extend_from_slice(&usd_token_address.to_bytes());
                buf.extend_from_slice(&fees_in_usd.to_le_bytes());
                buf.extend_from_slice(&company_wallet.to_bytes());
            }
            &Self::SetFeesInUSD {
                fees_in_usd,
            } => {
                buf.push(7);
                buf.extend_from_slice(&fees_in_usd.to_le_bytes());
            }
            &Self::SetCompanyWallet {
                company_wallet,
            } => {
                buf.push(8);
                buf.extend_from_slice(&company_wallet.to_bytes());
            }
            &Self::SetFreeToken {
                mint_address,
                is_free,
            } => {
                buf.push(9);
                buf.extend_from_slice(&mint_address.to_bytes());
                buf.extend_from_slice(&(is_free as u8).to_le_bytes());
            }
            &Self::TransferOwnership {} => {
                buf.push(10);
            }
        };
        buf
    }
}

pub fn init(
    system_program_id: &Pubkey,
    rent_program_id: &Pubkey,
    locking_program_id: &Pubkey,
    payer_key: &Pubkey,
    locking_account: &Pubkey,
    seeds: [u8; 32],
    number_of_schedules: u32,
) -> Result<Instruction, ProgramError> {
    let data = LockTokenInstruction::Init {
        seeds,
        number_of_schedules,
    }
    .pack();
    let accounts = vec![
        AccountMeta::new_readonly(*system_program_id, false),
        AccountMeta::new_readonly(*rent_program_id, false),
        AccountMeta::new(*payer_key, true),
        AccountMeta::new(*locking_account, false),
    ];
    Ok(Instruction {
        program_id: *locking_program_id,
        accounts,
        data,
    })
}

pub fn create(
    locking_program_id: &Pubkey,
    token_program_id: &Pubkey,
    locking_account_key: &Pubkey,
    locking_token_account_key: &Pubkey,
    source_token_account_owner_key: &Pubkey,
    source_token_account_key: &Pubkey,
    destination_token_account_key: &Pubkey,
    mint_address: &Pubkey,
    schedules: Vec<Schedule>,
    seeds: [u8; 32],
) -> Result<Instruction, ProgramError> {
    let data = LockTokenInstruction::Create {
        mint_address: *mint_address,
        seeds,
        destination_token_address: *destination_token_account_key,
        schedules,
    }
    .pack();
    let accounts = vec![
        AccountMeta::new_readonly(*token_program_id, false),
        AccountMeta::new(*locking_account_key, false),
        AccountMeta::new(*locking_token_account_key, false),
        AccountMeta::new_readonly(*source_token_account_owner_key, true),
        AccountMeta::new(*source_token_account_key, false),
    ];
    Ok(Instruction {
        program_id: *locking_program_id,
        accounts,
        data,
    })
}

pub fn unlock(
    locking_program_id: &Pubkey,
    token_program_id: &Pubkey,
    clock_sysvar_id: &Pubkey,
    locking_account_key: &Pubkey,
    locking_token_account_key: &Pubkey,
    destination_token_account_key: &Pubkey,
    seeds: [u8; 32],
) -> Result<Instruction, ProgramError> {
    let data = LockTokenInstruction::Unlock { seeds }.pack();
    let accounts = vec![
        AccountMeta::new_readonly(*token_program_id, false),
        AccountMeta::new_readonly(*clock_sysvar_id, false),
        AccountMeta::new(*locking_account_key, false),
        AccountMeta::new(*locking_token_account_key, false),
        AccountMeta::new(*destination_token_account_key, false),
    ];
    Ok(Instruction {
        program_id: *locking_program_id,
        accounts,
        data,
    })
}

pub fn transfer_locks(
    locking_program_id: &Pubkey,
    locking_account_key: &Pubkey,
    current_destination_token_account_owner: &Pubkey,
    current_destination_token_account: &Pubkey,
    target_destination_token_account: &Pubkey,
    seeds: [u8; 32],
) -> Result<Instruction, ProgramError> {
    let data = LockTokenInstruction::TransferLocks { seeds }.pack();
    let accounts = vec![
        AccountMeta::new(*locking_account_key, false),
        AccountMeta::new_readonly(*current_destination_token_account, false),
        AccountMeta::new_readonly(*current_destination_token_account_owner, true),
        AccountMeta::new_readonly(*target_destination_token_account, false),
    ];
    Ok(Instruction {
        program_id: *locking_program_id,
        accounts,
        data,
    })
}

pub fn extend_lock_duration(
    locking_program_id: &Pubkey,
    locking_account_key: &Pubkey,
    destination_token_account_owner: &Pubkey,
    destination_token_account: &Pubkey,
    seeds: [u8; 32],
    index: u32,
    release_time: u64,
) -> Result<Instruction, ProgramError> {
    let data = LockTokenInstruction::ExtendLockDuration { seeds, index, release_time }.pack();
    let accounts = vec![
        AccountMeta::new(*locking_account_key, false),
        AccountMeta::new_readonly(*destination_token_account, false),
        AccountMeta::new_readonly(*destination_token_account_owner, true),
    ];
    Ok(Instruction {
        program_id: *locking_program_id,
        accounts,
        data,
    })
}
