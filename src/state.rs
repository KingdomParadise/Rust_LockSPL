use solana_program::{
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
};

use std::convert::TryInto;

pub const OWNER_TOKEN_MINT_ADDRESS: &str = "Token address";

#[derive(Debug, PartialEq)]
pub struct LockGlobalState {
    pub price_estimator: Pubkey,
    pub usd_token_address: Pubkey,
    pub fees_in_usd: u64,
    pub company_wallet: Pubkey,
    pub is_paused: bool,
    pub is_initialized: bool,
}

#[derive(Debug, PartialEq)]
pub struct LockSchedule {
    pub release_time: u64,
    pub amount: u64,
}

#[derive(Debug, PartialEq)]
pub struct LockScheduleHeader {
    pub destination_address: Pubkey,
    pub mint_address: Pubkey,
    pub is_initialized: bool,
}

#[derive(Debug, PartialEq)]
pub struct TokenState {
    pub mint_address: Pubkey,
    pub is_free: bool,
    pub is_initialized: bool,
}

impl Sealed for LockScheduleHeader {}

impl Pack for LockScheduleHeader {
    const LEN: usize = 65;

    fn pack_into_slice(&self, target: &mut [u8]) {
        let destination_address_bytes = self.destination_address.to_bytes();
        let mint_address_bytes = self.mint_address.to_bytes();
        for i in 0..32 {
            target[i] = destination_address_bytes[i];
        }

        for i in 32..64 {
            target[i] = mint_address_bytes[i - 32];
        }

        target[64] = self.is_initialized as u8;
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        if src.len() < Self::LEN {
            return Err(ProgramError::InvalidAccountData)
        }
        let destination_address = Pubkey::new(&src[..32]);
        let mint_address = Pubkey::new(&src[32..64]);
        let is_initialized = src[64] == 1;
        Ok(Self {
            destination_address,
            mint_address,
            is_initialized,
        })
    }
}

impl IsInitialized for LockScheduleHeader {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl Sealed for LockSchedule {}

impl Pack for LockSchedule {
    const LEN: usize = 16;

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let release_time_bytes = self.release_time.to_le_bytes();
        let amount_bytes = self.amount.to_le_bytes();
        for i in 0..8 {
            dst[i] = release_time_bytes[i];
        }

        for i in 8..16 {
            dst[i] = amount_bytes[i - 8];
        }
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        if src.len() < 16 {
            return Err(ProgramError::InvalidAccountData)
        }
        let release_time = u64::from_le_bytes(src[0..8].try_into().unwrap());
        let amount = u64::from_le_bytes(src[8..16].try_into().unwrap());
        Ok(Self {
            release_time,
            amount,
        })
    }
}

impl IsInitialized for LockSchedule {
    fn is_initialized(&self) -> bool {
        self.amount > 0
    }
}

pub fn unpack_schedules(input: &[u8]) -> Result<Vec<LockSchedule>, ProgramError> {
    let number_of_schedules = input.len() / LockSchedule::LEN;
    let mut output: Vec<LockSchedule> = Vec::with_capacity(number_of_schedules);
    let mut offset = 0;
    for _ in 0..number_of_schedules {
        output.push(LockSchedule::unpack_from_slice(
            &input[offset..offset + LockSchedule::LEN],
        )?);
        offset += LockSchedule::LEN;
    }
    Ok(output)
}

pub fn pack_schedules_into_slice(schedules: Vec<LockSchedule>, target: &mut [u8]) {
    let mut offset = 0;
    for s in schedules.iter() {
        s.pack_into_slice(&mut target[offset..]);
        offset += LockSchedule::LEN;
    }
}

impl Sealed for TokenState {}

impl Pack for TokenState {
    const LEN: usize = 34;

    fn pack_into_slice(&self, target: &mut [u8]) {
        let mint_address_bytes = self.mint_address.to_bytes();

        for i in 0..32 {
            target[i] = mint_address_bytes[i];
        }

        target[32] = self.is_free as u8;
        target[33] = self.is_initialized as u8;
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        if src.len() < Self::LEN {
            return Err(ProgramError::InvalidAccountData)
        }

        let mint_address = Pubkey::new(&src[..32]);
        let is_free = src[32] == 1;
        let is_initialized = src[33] == 1;

        Ok(Self {
            mint_address,
            is_free,
            is_initialized,
        })
    }
}

impl IsInitialized for TokenState {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl TokenState {
    pub fn estimate_fees_in_sol(&self) -> Result<u64, ProgramError> {
        if self.is_free == false {
            return Ok(0);
        }
        Ok(100)
    }
}

impl Sealed for LockGlobalState {}

impl Pack for LockGlobalState {
    const LEN: usize = 106;

    fn pack_into_slice(&self, target: &mut [u8]) {
        let price_estimator_bytes = self.price_estimator.to_bytes();
        let usd_token_address_bytes = self.usd_token_address.to_bytes();
        let fees_in_usd_bytes = self.fees_in_usd.to_le_bytes();
        let company_wallet_bytes = self.company_wallet.to_bytes();
        
        for i in 0..32 {
            target[i] = price_estimator_bytes[i];
        }

        for i in 32..64 {
            target[i] = usd_token_address_bytes[i - 32];
        }

        for i in 64..72 {
            target[i] = fees_in_usd_bytes[i - 64];
        }

        for i in 72..104 {
            target[i] = company_wallet_bytes[i - 72];
        }

        target[104] = self.is_paused as u8;
        target[105] = self.is_initialized as u8;
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        if src.len() < Self::LEN {
            return Err(ProgramError::InvalidAccountData)
        }

        let price_estimator = Pubkey::new(&src[..32]);
        let usd_token_address = Pubkey::new(&src[32..64]);
        let fees_in_usd = u64::from_le_bytes(src[64..72].try_into().unwrap());
        let company_wallet = Pubkey::new(&src[72..104]);
        let is_paused = src[104] == 1;
        let is_initialized = src[105] == 1;

        Ok(Self {
            price_estimator,
            usd_token_address,
            fees_in_usd,
            company_wallet,
            is_paused,
            is_initialized,
        })
    }
}

impl IsInitialized for LockGlobalState {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}