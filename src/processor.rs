use solana_program::{
    account_info::{next_account_info, AccountInfo},
    decode_error::DecodeError,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::PrintProgramError,
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction::{create_account, transfer as transfer_sol},
    sysvar::{clock::Clock, Sysvar},
};

use std::str::FromStr;

use num_traits::FromPrimitive;
use spl_token::{instruction::transfer, state::Account};

use crate::{
    error::LockTokenError,
    instruction::{Schedule, LockTokenInstruction, SCHEDULE_SIZE},
    state::{OWNER_TOKEN_MINT_ADDRESS, pack_schedules_into_slice, unpack_schedules, LockGlobalState, LockSchedule, LockScheduleHeader, TokenState},
};

pub struct Processor {}

impl Processor {
    pub fn process_init(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        seeds: [u8; 32],
        schedules: u32
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let system_program_account = next_account_info(accounts_iter)?;
        let program_state_account = next_account_info(accounts_iter)?;
        let rent_sysvar_account = next_account_info(accounts_iter)?;
        let payer = next_account_info(accounts_iter)?;
        let locking_account = next_account_info(accounts_iter)?;

        let rent = Rent::from_account_info(rent_sysvar_account)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            msg!("The state of program is uninitialized");
            return Err(ProgramError::InvalidArgument);
        }

        let program_global_state = LockGlobalState::unpack(&program_state_account.data.borrow())?;

        if program_global_state.is_paused {
            msg!("The program is paused");
            return Err(ProgramError::InvalidArgument);
        }

        let locking_account_key = Pubkey::create_program_address(&[&seeds], &program_id).unwrap();
        if locking_account_key != *locking_account.key {
            msg!("Provided locking account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let state_size = (schedules as usize) * LockSchedule::LEN + LockScheduleHeader::LEN;

        let init_locking_account = create_account(
            &payer.key,
            &locking_account_key,
            rent.minimum_balance(state_size),
            state_size as u64,
            &program_id,
        );

        invoke_signed(
            &init_locking_account,
            &[
                system_program_account.clone(),
                payer.clone(),
                locking_account.clone(),
            ],
            &[&[&seeds]],
        )?;
        Ok(())
    }

    pub fn process_create(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        seeds: [u8; 32],
        mint_address: &Pubkey,
        destination_token_address: &Pubkey,
        schedules: Vec<Schedule>,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let spl_token_account = next_account_info(accounts_iter)?;
        let program_state_account = next_account_info(accounts_iter)?;
        let locking_account = next_account_info(accounts_iter)?;
        let locking_token_account = next_account_info(accounts_iter)?;
        let source_token_account_owner = next_account_info(accounts_iter)?;
        let source_token_account = next_account_info(accounts_iter)?;
        let token_state_account = next_account_info(accounts_iter)?;
        let company_wallet = next_account_info(accounts_iter)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            msg!("The state of program is uninitialized");
            return Err(ProgramError::InvalidArgument);
        }

        let program_global_state = LockGlobalState::unpack(&program_state_account.data.borrow())?;

        if program_global_state.is_paused {
            msg!("The program is paused");
            return Err(ProgramError::InvalidArgument);
        }

        let locking_account_key = Pubkey::create_program_address(&[&seeds], program_id)?;
        if locking_account_key != *locking_account.key {
            msg!("Provided locking account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        if !source_token_account_owner.is_signer {
            msg!("Source token account owner should be a signer.");
            return Err(ProgramError::InvalidArgument);
        }

        if *locking_account.owner != *program_id {
            msg!("Program should own locking account");
            return Err(ProgramError::InvalidArgument);
        }

        // Verifying that no SVC was already created with this seed
        let is_initialized =
            locking_account.try_borrow_data()?[LockScheduleHeader::LEN - 1] == 1;

        if is_initialized {
            msg!("Cannot overwrite an existing locking contract.");
            return Err(ProgramError::InvalidArgument);
        }

        let locking_token_account_data = Account::unpack(&locking_token_account.data.borrow())?;

        if locking_token_account_data.owner != locking_account_key {
            msg!("The locking token account should be owned by the locking account.");
            return Err(ProgramError::InvalidArgument);
        }

        if locking_token_account_data.delegate.is_some() {
            msg!("The locking token account should not have a delegate authority");
            return Err(ProgramError::InvalidAccountData);
        }

        if locking_token_account_data.close_authority.is_some() {
            msg!("The locking token account should not have a close authority");
            return Err(ProgramError::InvalidAccountData);
        }

        let token_state_account_key = Pubkey::create_program_address(&[&mint_address.to_bytes()], program_id)?;
        if token_state_account_key != *token_state_account.key {
            msg!("Provided token state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let mut token_state_data = TokenState {
            mint_address: *mint_address,
            is_free: false,
            is_initialized: false,
        };
        let is_free_token_initialized = token_state_account.try_borrow_data()?[TokenState::LEN - 1] == 1;
        if is_free_token_initialized == true {
            let packed_state = &token_state_account.data;
            token_state_data = TokenState::unpack(&packed_state.borrow()[..TokenState::LEN])?;
            if token_state_data.mint_address != *mint_address {
                msg!("Provided token state account is invalid");
                return Err(ProgramError::InvalidArgument);
            }
        }
        
        let transfer_sol_to_company_wallet = transfer_sol(
            &source_token_account_owner.key,
            &company_wallet.key,
            token_state_data.estimate_fees_in_sol()?,
        );

        invoke(
            &transfer_sol_to_company_wallet,
            &[
                source_token_account_owner.clone(),
                company_wallet.clone(),
            ],
        )?;

        let state_header = LockScheduleHeader {
            destination_address: *destination_token_address,
            mint_address: *mint_address,
            is_initialized: true,
        };

        let mut data = locking_account.data.borrow_mut();
        if data.len() != LockScheduleHeader::LEN + schedules.len() * LockSchedule::LEN {
            return Err(ProgramError::InvalidAccountData)
        }
        state_header.pack_into_slice(&mut data);

        let mut offset = LockScheduleHeader::LEN;
        let mut total_amount: u64 = 0;

        for s in schedules.iter() {
            let state_schedule = LockSchedule {
                release_time: s.release_time,
                amount: s.amount,
            };
            state_schedule.pack_into_slice(&mut data[offset..]);
            let delta = total_amount.checked_add(s.amount);
            match delta {
                Some(n) => total_amount = n,
                None => return Err(ProgramError::InvalidInstructionData), // Total amount overflows u64
            }
            offset += SCHEDULE_SIZE;
        }
        
        if Account::unpack(&source_token_account.data.borrow())?.amount < total_amount {
            msg!("The source token account has insufficient funds.");
            return Err(ProgramError::InsufficientFunds)
        };

        let transfer_tokens_to_locking_account = transfer(
            spl_token_account.key,
            source_token_account.key,
            locking_token_account.key,
            source_token_account_owner.key,
            &[],
            total_amount,
        )?;

        invoke(
            &transfer_tokens_to_locking_account,
            &[
                source_token_account.clone(),
                locking_token_account.clone(),
                spl_token_account.clone(),
                source_token_account_owner.clone(),
            ],
        )?;
        Ok(())
    }

    pub fn process_unlock(
        program_id: &Pubkey,
        _accounts: &[AccountInfo],
        seeds: [u8; 32],
    ) -> ProgramResult {
        let accounts_iter = &mut _accounts.iter();

        let spl_token_account = next_account_info(accounts_iter)?;
        let clock_sysvar_account = next_account_info(accounts_iter)?;
        let program_state_account = next_account_info(accounts_iter)?;
        let locking_account = next_account_info(accounts_iter)?;
        let locking_token_account = next_account_info(accounts_iter)?;
        let destination_token_account = next_account_info(accounts_iter)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            msg!("The state of program is uninitialized");
            return Err(ProgramError::InvalidArgument);
        }

        let program_global_state = LockGlobalState::unpack(&program_state_account.data.borrow())?;

        if program_global_state.is_paused {
            msg!("The program is paused");
            return Err(ProgramError::InvalidArgument);
        }

        let locking_account_key = Pubkey::create_program_address(&[&seeds], program_id)?;
        if locking_account_key != *locking_account.key {
            msg!("Invalid locking account key");
            return Err(ProgramError::InvalidArgument);
        }

        if spl_token_account.key != &spl_token::id() {
            msg!("The provided spl token program account is invalid");
            return Err(ProgramError::InvalidArgument)
        }

        let packed_state = &locking_account.data;
        let header_state =
            LockScheduleHeader::unpack(&packed_state.borrow()[..LockScheduleHeader::LEN])?;

        if header_state.destination_address != *destination_token_account.key {
            msg!("Contract destination account does not matched provided account");
            return Err(ProgramError::InvalidArgument);
        }

        let locking_token_account_data = Account::unpack(&locking_token_account.data.borrow())?;

        if locking_token_account_data.owner != locking_account_key {
            msg!("The locking token account should be owned by the locking account.");
            return Err(ProgramError::InvalidArgument);
        }

        // Unlock the schedules that have reached maturity
        let clock = Clock::from_account_info(&clock_sysvar_account)?;
        let mut total_amount_to_transfer = 0;
        let mut schedules = unpack_schedules(&packed_state.borrow()[LockScheduleHeader::LEN..])?;

        for s in schedules.iter_mut() {
            if clock.unix_timestamp as u64 >= s.release_time {
                total_amount_to_transfer += s.amount;
                s.amount = 0;
            }
        }
        if total_amount_to_transfer == 0 {
            msg!("locking contract has not yet reached release time");
            return Err(ProgramError::InvalidArgument);
        }

        let transfer_tokens_from_locking_account = transfer(
            &spl_token_account.key,
            &locking_token_account.key,
            destination_token_account.key,
            &locking_account_key,
            &[],
            total_amount_to_transfer,
        )?;

        invoke_signed(
            &transfer_tokens_from_locking_account,
            &[
                spl_token_account.clone(),
                locking_token_account.clone(),
                destination_token_account.clone(),
                locking_account.clone(),
            ],
            &[&[&seeds]],
        )?;

        // Reset released amounts to 0. This makes the simple unlock safe with complex scheduling contracts
        pack_schedules_into_slice(
            schedules,
            &mut packed_state.borrow_mut()[LockScheduleHeader::LEN..],
        );

        Ok(())
    }

    pub fn process_transfer_locks(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        seeds: [u8; 32],
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let program_state_account = next_account_info(accounts_iter)?;
        let locking_account = next_account_info(accounts_iter)?;
        let destination_token_account = next_account_info(accounts_iter)?;
        let destination_token_account_owner = next_account_info(accounts_iter)?;
        let new_destination_token_account = next_account_info(accounts_iter)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            msg!("The state of program is uninitialized");
            return Err(ProgramError::InvalidArgument);
        }

        let program_global_state = LockGlobalState::unpack(&program_state_account.data.borrow())?;

        if program_global_state.is_paused {
            msg!("The program is paused");
            return Err(ProgramError::InvalidArgument);
        }

        if locking_account.data.borrow().len() < LockScheduleHeader::LEN {
            return Err(ProgramError::InvalidAccountData)
        }
        let locking_account_key = Pubkey::create_program_address(&[&seeds], program_id)?;
        let state = LockScheduleHeader::unpack(
            &locking_account.data.borrow()[..LockScheduleHeader::LEN],
        )?;

        if locking_account_key != *locking_account.key {
            msg!("Invalid locking account key");
            return Err(ProgramError::InvalidArgument);
        }

        if state.destination_address != *destination_token_account.key {
            msg!("Contract destination account does not matched provided account");
            return Err(ProgramError::InvalidArgument);
        }

        if !destination_token_account_owner.is_signer {
            msg!("Destination token account owner should be a signer.");
            return Err(ProgramError::InvalidArgument);
        }

        let destination_token_account = Account::unpack(&destination_token_account.data.borrow())?;

        if destination_token_account.owner != *destination_token_account_owner.key {
            msg!("The current destination token account isn't owned by the provided owner");
            return Err(ProgramError::InvalidArgument);
        }

        let mut new_state = state;
        new_state.destination_address = *new_destination_token_account.key;
        new_state
            .pack_into_slice(&mut locking_account.data.borrow_mut()[..LockScheduleHeader::LEN]);

        Ok(())
    }

    pub fn process_extend_lock_duration(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        seeds: [u8; 32],
        index: u32,
        release_time: u64,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let program_state_account = next_account_info(accounts_iter)?;
        let locking_account = next_account_info(accounts_iter)?;
        let destination_token_account = next_account_info(accounts_iter)?;
        let destination_token_account_owner = next_account_info(accounts_iter)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            msg!("The state of program is uninitialized");
            return Err(ProgramError::InvalidArgument);
        }

        let program_global_state = LockGlobalState::unpack(&program_state_account.data.borrow())?;

        if program_global_state.is_paused {
            msg!("The program is paused");
            return Err(ProgramError::InvalidArgument);
        }

        if locking_account.data.borrow().len() < LockScheduleHeader::LEN + LockSchedule::LEN * (index as usize + 1) {
            return Err(ProgramError::InvalidAccountData)
        }
        let locking_account_key = Pubkey::create_program_address(&[&seeds], program_id)?;
        let state = LockSchedule::unpack(
            &locking_account.data.borrow()[(LockScheduleHeader::LEN + LockSchedule::LEN * index as usize)..(LockScheduleHeader::LEN + LockSchedule::LEN * (index as usize + 1))],
        )?;

        if locking_account_key != *locking_account.key {
            msg!("Invalid locking account key");
            return Err(ProgramError::InvalidArgument);
        }

        if state.release_time > release_time {
            msg!("Can not set shorter release time.");
            return Err(ProgramError::InvalidArgument);
        }

        if !destination_token_account_owner.is_signer {
            msg!("Destination token account owner should be a signer.");
            return Err(ProgramError::InvalidArgument);
        }

        let destination_token_account = Account::unpack(&destination_token_account.data.borrow())?;

        if destination_token_account.owner != *destination_token_account_owner.key {
            msg!("The current destination token account isn't owned by the provided owner");
            return Err(ProgramError::InvalidArgument);
        }

        let mut new_state = state;
        new_state.release_time = release_time;
        new_state
            .pack_into_slice(&mut locking_account.data.borrow_mut()[(LockScheduleHeader::LEN + LockSchedule::LEN * index as usize)..(LockScheduleHeader::LEN + LockSchedule::LEN * (index as usize + 1))]);

        Ok(())
    }

    pub fn process_pause_contract(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        is_pause: bool,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let program_owner_account = next_account_info(accounts_iter)?;
        let program_owner_token_account = next_account_info(accounts_iter)?;
        let program_state_account = next_account_info(accounts_iter)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        if !program_owner_account.is_signer {
            msg!("Program owner account should be a signer");
            return Err(ProgramError::InvalidArgument);
        }

        if *program_state_account.owner != *program_id {
            msg!("Program should own program state account");
            return Err(ProgramError::InvalidArgument);
        }

        let program_owner_token_account_data = Account::unpack(&program_owner_token_account.data.borrow())?;

        if program_owner_token_account_data.owner != *program_owner_account.key {
            msg!("Program owner account should own token account.");
            return Err(ProgramError::InvalidArgument);
        }

        let owner_token_mint_key = Pubkey::from_str(OWNER_TOKEN_MINT_ADDRESS);
        match owner_token_mint_key {
            Ok(v) => { 
                if (v != program_owner_token_account_data.mint) || (program_owner_token_account_data.amount == 0) {
                    msg!("Program owner account shold own the specified owner token mint.");
                    return Err(ProgramError::InvalidArgument);
                }
            },
            Err(_e) => {
                msg!("Program owner account shold own the specified owner token mint.");
                return Err(ProgramError::InvalidArgument);
            },
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            msg!("The state of program is uninitialized");
            return Err(ProgramError::InvalidArgument);
        }

        let packed_state_data = &program_state_account.data;
        let mut program_global_state = LockGlobalState::unpack(&packed_state_data.borrow()[..LockGlobalState::LEN])?;

        program_global_state.is_paused = is_pause;
        program_global_state.pack_into_slice(&mut program_state_account.data.borrow_mut()[..]);

        Ok(())
    }

    pub fn process_set_fee_params(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        price_estimator: &Pubkey,
        usd_token_address: &Pubkey,
        fees_in_usd: u64,
        company_wallet: &Pubkey,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let system_program_account = next_account_info(accounts_iter)?;
        let rent_sysvar_account = next_account_info(accounts_iter)?;
        let program_owner_account = next_account_info(accounts_iter)?;
        let program_owner_token_account = next_account_info(accounts_iter)?;
        let program_state_account = next_account_info(accounts_iter)?;

        let rent = Rent::from_account_info(rent_sysvar_account)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        if !program_owner_account.is_signer {
            msg!("Program owner account should be a signer");
            return Err(ProgramError::InvalidArgument);
        }

        if *program_state_account.owner != *program_id {
            msg!("Program should own program state account");
            return Err(ProgramError::InvalidArgument);
        }

        let program_owner_token_account_data = Account::unpack(&program_owner_token_account.data.borrow())?;

        if program_owner_token_account_data.owner != *program_owner_account.key {
            msg!("Program owner account should own token account.");
            return Err(ProgramError::InvalidArgument);
        }

        let owner_token_mint_key = Pubkey::from_str(OWNER_TOKEN_MINT_ADDRESS);
        match owner_token_mint_key {
            Ok(v) => { 
                if (v != program_owner_token_account_data.mint) || (program_owner_token_account_data.amount == 0) {
                    msg!("Program owner account shold own the specified owner token mint.");
                    return Err(ProgramError::InvalidArgument);
                }
            },
            Err(_e) => {
                msg!("Program owner account shold own the specified owner token mint.");
                return Err(ProgramError::InvalidArgument);
            },
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            let create_program_state_account = create_account(
                &program_owner_account.key,
                &program_state_account_key,
                rent.minimum_balance(LockGlobalState::LEN),
                LockGlobalState::LEN as u64,
                &program_id,
            );
    
            invoke_signed(
                &create_program_state_account,
                &[
                    system_program_account.clone(),
                    program_owner_account.clone(),
                    program_state_account.clone(),
                ],
                &[&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()]],
            )?;
        }

        let mut program_state_data = LockGlobalState::unpack(&program_state_account.data.borrow())?;
        program_state_data.price_estimator = *price_estimator;
        program_state_data.usd_token_address = *usd_token_address;
        program_state_data.fees_in_usd = fees_in_usd;
        program_state_data.company_wallet = *company_wallet;

        program_state_data.pack_into_slice(&mut program_state_account.data.borrow_mut());

        Ok(())
    }

    pub fn process_set_fees_in_usd(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        fees_in_usd: u64,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let program_owner_account = next_account_info(accounts_iter)?;
        let program_owner_token_account = next_account_info(accounts_iter)?;
        let program_state_account = next_account_info(accounts_iter)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        if !program_owner_account.is_signer {
            msg!("Program owner account should be a signer");
            return Err(ProgramError::InvalidArgument);
        }

        if *program_state_account.owner != *program_id {
            msg!("Program should own program state account");
            return Err(ProgramError::InvalidArgument);
        }

        let program_owner_token_account_data = Account::unpack(&program_owner_token_account.data.borrow())?;

        if program_owner_token_account_data.owner != *program_owner_account.key {
            msg!("Program owner account should own token account.");
            return Err(ProgramError::InvalidArgument);
        }

        let owner_token_mint_key = Pubkey::from_str(OWNER_TOKEN_MINT_ADDRESS);
        match owner_token_mint_key {
            Ok(v) => { 
                if (v != program_owner_token_account_data.mint) || (program_owner_token_account_data.amount == 0) {
                    msg!("Program owner account shold own the specified owner token mint.");
                    return Err(ProgramError::InvalidArgument);
                }
            },
            Err(_e) => {
                msg!("Program owner account shold own the specified owner token mint.");
                return Err(ProgramError::InvalidArgument);
            },
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            msg!("The state of program is uninitialized");
            return Err(ProgramError::InvalidArgument);
        }

        let mut program_state_data = LockGlobalState::unpack(&program_state_account.data.borrow())?;
        program_state_data.fees_in_usd = fees_in_usd;

        program_state_data.pack_into_slice(&mut program_state_account.data.borrow_mut()[..]);

        Ok(())
    }

    pub fn process_set_company_wallet(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        company_wallet: &Pubkey,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let program_owner_account = next_account_info(accounts_iter)?;
        let program_owner_token_account = next_account_info(accounts_iter)?;
        let program_state_account = next_account_info(accounts_iter)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        if !program_owner_account.is_signer {
            msg!("Program owner account should be a signer");
            return Err(ProgramError::InvalidArgument);
        }

        if *program_state_account.owner != *program_id {
            msg!("Program should own program state account");
            return Err(ProgramError::InvalidArgument);
        }

        let program_owner_token_account_data = Account::unpack(&program_owner_token_account.data.borrow())?;

        if program_owner_token_account_data.owner != *program_owner_account.key {
            msg!("Program owner account should own token account.");
            return Err(ProgramError::InvalidArgument);
        }

        let owner_token_mint_key = Pubkey::from_str(OWNER_TOKEN_MINT_ADDRESS);
        match owner_token_mint_key {
            Ok(v) => { 
                if (v != program_owner_token_account_data.mint) || (program_owner_token_account_data.amount == 0) {
                    msg!("Program owner account shold own the specified owner token mint.");
                    return Err(ProgramError::InvalidArgument);
                }
            },
            Err(_e) => {
                msg!("Program owner account shold own the specified owner token mint.");
                return Err(ProgramError::InvalidArgument);
            },
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            msg!("The state of program is uninitialized");
            return Err(ProgramError::InvalidArgument);
        }

        let mut program_state_data = LockGlobalState::unpack(&program_state_account.data.borrow())?;
        program_state_data.company_wallet = *company_wallet;

        program_state_data.pack_into_slice(&mut program_state_account.data.borrow_mut()[..]);

        Ok(())
    }

    pub fn process_set_free_token(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        mint_address: &Pubkey,
        is_free: bool,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let program_owner_account = next_account_info(accounts_iter)?;
        let program_owner_token_account = next_account_info(accounts_iter)?;
        let program_state_account = next_account_info(accounts_iter)?;
        let token_state_account = next_account_info(accounts_iter)?;

        let program_state_account_key = Pubkey::create_program_address(&[String::from(OWNER_TOKEN_MINT_ADDRESS).as_bytes()], program_id)?;

        if program_state_account_key != *program_state_account.key {
            msg!("Provided program state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        if !program_owner_account.is_signer {
            msg!("Program owner account should be a signer");
            return Err(ProgramError::InvalidArgument);
        }

        if *program_state_account.owner != *program_id {
            msg!("Program should own program state account");
            return Err(ProgramError::InvalidArgument);
        }

        let program_owner_token_account_data = Account::unpack(&program_owner_token_account.data.borrow())?;

        if program_owner_token_account_data.owner != *program_owner_account.key {
            msg!("Program owner account should own token account.");
            return Err(ProgramError::InvalidArgument);
        }

        let owner_token_mint_key = Pubkey::from_str(OWNER_TOKEN_MINT_ADDRESS);
        match owner_token_mint_key {
            Ok(v) => { 
                if (v != program_owner_token_account_data.mint) || (program_owner_token_account_data.amount == 0) {
                    msg!("Program owner account shold own the specified owner token mint.");
                    return Err(ProgramError::InvalidArgument);
                }
            },
            Err(_e) => {
                msg!("Program owner account shold own the specified owner token mint.");
                return Err(ProgramError::InvalidArgument);
            },
        }

        let is_state_initialized = program_state_account.try_borrow_data()?[LockGlobalState::LEN - 1] == 1;

        if !is_state_initialized {
            msg!("The state of program is uninitialized");
            return Err(ProgramError::InvalidArgument);
        }

        let packed_state_data = &program_state_account.data;
        let program_global_state = LockGlobalState::unpack(&packed_state_data.borrow()[..LockGlobalState::LEN])?;

        if program_global_state.is_paused {
            msg!("The program is paused");
            return Err(ProgramError::InvalidArgument);
        }

        let token_state_account_key = Pubkey::create_program_address(&[&mint_address.to_bytes()], program_id)?;
        if token_state_account_key != *token_state_account.key {
            msg!("Provided token state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let mut token_state_data = TokenState::unpack(&token_state_account.data.borrow())?;
        
        if token_state_data.mint_address != *mint_address {
            msg!("Provided token state account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        token_state_data.is_free = is_free;
        token_state_data.pack_into_slice(&mut token_state_account.data.borrow_mut()[..]);

        Ok(())
    }

    pub fn process_transfer_ownership(
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let spl_token_account = next_account_info(accounts_iter)?;
        let old_owner_account = next_account_info(accounts_iter)?;
        let old_owner_token_account = next_account_info(accounts_iter)?;
        let new_owner_account = next_account_info(accounts_iter)?;
        let new_owner_token_account = next_account_info(accounts_iter)?;

        if !old_owner_account.is_signer {
            msg!("Old owner account should be a signer");
            return Err(ProgramError::InvalidArgument);
        }

        let old_owner_token_account_data = Account::unpack(&old_owner_token_account.data.borrow())?;
        if old_owner_token_account_data.owner != *old_owner_account.key {
            msg!("Old owner account and token account are invalid");
            return Err(ProgramError::InvalidArgument);
        }

        if old_owner_token_account_data.amount == 0 {
            msg!("Old owner has no ownership");
            return Err(ProgramError::InvalidArgument);
        }

        let new_owner_token_account_data = Account::unpack(&new_owner_token_account.data.borrow())?;
        if new_owner_token_account_data.owner != *new_owner_account.key {
            msg!("New owner account and token account are invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let transfer_owner_token = transfer(
            spl_token_account.key,
            old_owner_token_account.key,
            new_owner_token_account.key,
            old_owner_account.key,
            &[],
            1,
        )?;

        invoke(
            &transfer_owner_token,
            &[
                old_owner_token_account.clone(),
                new_owner_token_account.clone(),
                spl_token_account.clone(),
                old_owner_account.clone(),
            ],
        )?;

        Ok(())
    }

    pub fn process_instruction(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        msg!("Beginning processing");
        let instruction = LockTokenInstruction::unpack(instruction_data)?;
        msg!("Instruction unpacked");
        match instruction {
            LockTokenInstruction::Init {
                seeds,
                number_of_schedules,
            } => {
                msg!("Instruction: Init");
                Self::process_init(program_id, accounts, seeds, number_of_schedules)
            }
            LockTokenInstruction::Unlock { seeds } => {
                msg!("Instruction: Unlock");
                Self::process_unlock(program_id, accounts, seeds)
            }
            LockTokenInstruction::TransferLocks { seeds } => {
                msg!("Instruction: Transfer Locks");
                Self::process_transfer_locks(program_id, accounts, seeds)
            }
            LockTokenInstruction::Create {
                seeds,
                mint_address,
                destination_token_address,
                schedules,
            } => {
                msg!("Instruction: Create Schedule");
                Self::process_create(
                    program_id,
                    accounts,
                    seeds,
                    &mint_address,
                    &destination_token_address,
                    schedules,
                )
            }
            LockTokenInstruction::ExtendLockDuration {
                seeds,
                index,
                release_time,
            } => {
                msg!("Instruction: Extend Lock Duration");
                Self::process_extend_lock_duration(
                    program_id,
                    accounts,
                    seeds,
                    index,
                    release_time,
                )
            }
            LockTokenInstruction::PauseContract {
                is_pause,
            } => {
                msg!("Instruction: Pause program: {}", is_pause);
                Self::process_pause_contract(
                    program_id,
                    accounts,
                    is_pause
                )
            }
            LockTokenInstruction::SetFeeParams {
                price_estimator,
                usd_token_address,
                fees_in_usd,
                company_wallet,
            } => {
                msg!("Instruction: Set Fee Params");
                Self::process_set_fee_params(
                    program_id,
                    accounts,
                    &price_estimator,
                    &usd_token_address,
                    fees_in_usd,
                    &company_wallet,
                )
            }
            LockTokenInstruction::SetFeesInUSD {
                fees_in_usd,
            } => {
                msg!("Instruction: Set Fees In USD");
                Self::process_set_fees_in_usd(
                    program_id,
                    accounts,
                    fees_in_usd,
                )
            }
            LockTokenInstruction::SetCompanyWallet {
                company_wallet,
            } => {
                msg!("Instruction: Set Company Wallet");
                Self::process_set_company_wallet(
                    program_id,
                    accounts,
                    &company_wallet,
                )
            }
            LockTokenInstruction::SetFreeToken {
                mint_address,
                is_free,
            } => {
                msg!("Instruction: Set Free Token");
                Self::process_set_free_token(
                    program_id,
                    accounts,
                    &mint_address,
                    is_free,
                )
            }
            LockTokenInstruction::TransferOwnership {} => {
                msg!("Instruction: Transfer Ownership");
                Self::process_transfer_ownership(
                    accounts,
                )
            }
        }
    }
}

impl PrintProgramError for LockTokenError {
    fn print<E>(&self)
    where
        E: 'static + std::error::Error + DecodeError<E> + PrintProgramError + FromPrimitive,
    {
        match self {
            LockTokenError::InvalidInstruction => msg!("Error: Invalid instruction!"),
        }
    }
}
