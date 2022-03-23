// Copyright 2019, 2020 Finastate
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::HashMap;
use std::ffi::c_void;

use wasmer_runtime::Memory;
use wasmer_runtime::{func, imports};
use wasmer_runtime_core::import::ImportObject;
use wasmer_runtime_core::vm::Ctx;

use primitives::{Address, Balance, Event};

use crate::errors::{ApplicationError, ContractError, VMError, VMResult};
use crate::{VMConfig, VMContext};

pub struct State<'a> {
	pub config: &'a VMConfig,
	pub memory: Memory,
	pub shares: HashMap<u64, Vec<u8>>,
	pub context: &'a dyn VMContext,
	pub method: &'a str,
	pub params: &'a [u8],
	pub pay_value: Balance,
	pub result: Option<Vec<u8>>,
}

impl<'a> State<'a> {
	fn from_ctx(ctx: &mut Ctx) -> &mut Self {
		unsafe { &mut *(ctx.data as *mut State) }
	}

	fn memory_to_vec(&self, len: u64, ptr: u64) -> Vec<u8> {
		let ptr = ptr as usize;
		let len = len as usize;
		let mut data = vec![0u8; len];
		for (i, cell) in self.memory.view()[ptr..(ptr + len)].iter().enumerate() {
			data[i] = cell.get();
		}
		data
	}

	fn vec_to_memory(&self, ptr: u64, data: &[u8]) {
		let ptr = ptr as usize;
		self.memory.view()[ptr..(ptr + data.len())]
			.iter()
			.zip(data.iter())
			.for_each(|(cell, v)| cell.set(*v));
	}

	fn share_to_vec(&self, share_id: u64) -> VMResult<&Vec<u8>> {
		let data = self
			.shares
			.get(&share_id)
			.ok_or(ContractError::ShareIllegalAccess)?;
		Ok(data)
	}

	fn share_to_len(&self, share_id: u64) -> u64 {
		let len = self
			.shares
			.get(&share_id)
			.map(|value| value.len() as _)
			.unwrap_or(u64::MAX);
		len
	}

	fn vec_to_share(&mut self, share_id: u64, data: Vec<u8>) -> VMResult<()> {
		if data.len() as u64 > self.config.max_share_value_len {
			return Err(ContractError::ShareValueLenExceeded.into());
		}
		self.shares.insert(share_id, data);
		if self.shares.len() as u64 > self.config.max_share_size {
			return Err(ContractError::ShareSizeExceeded.into());
		}
		Ok(())
	}

	fn make_error_aware<T>(&mut self, result: VMResult<T>, error_share_id: u64) -> VMResult<u64> {
		match result {
			Ok(_) => Ok(0),
			Err(VMError::System(e)) => Err(VMError::System(e)),
			Err(VMError::Application(e)) => {
				let e = e.to_string();
				let e = e.into_bytes();
				self.vec_to_share(error_share_id, e)?;
				Ok(1)
			}
		}
	}
}

struct StateRef(*mut c_void);

unsafe impl Send for StateRef {}

unsafe impl Sync for StateRef {}

pub fn import(state: &mut State, memory: Memory) -> VMResult<ImportObject> {
	let state_ref = StateRef(state as *mut _ as *mut c_void);

	let import_object = imports! {
		move || (state_ref.0, |_a| {}),
		"env" => {
			"memory" => memory,
			"share_read" => func!(share_read),
			"share_len" => func!(share_len),
			"share_write" => func!(share_write),
			"method_read" => func!(method_read),
			"params_read" => func!(params_read),
			"pay_value_read" => func!(pay_value_read),
			"result_write" => func!(result_write),
			"error_return" => func!(error_return),
			"abort" => func!(abort),
			"env_block_number" => func!(env_block_number),
			"env_block_timestamp" => func!(env_block_timestamp),
			"env_tx_hash_read" => func!(env_tx_hash_read),
			"env_contract_address_read" => func!(env_contract_address_read),
			"env_sender_address_read" => func!(env_sender_address_read),
			"storage_read" => func!(storage_read),
			"storage_write" => func!(storage_write),
			"event_write" => func!(event_write),
			"util_hash" => func!(util_hash),
			"util_address" => func!(util_address),
			"util_validate_address" => func!(util_validate_address),
			"util_validate_address_ea" => func!(util_validate_address_ea),
			"balance_read" => func!(balance_read),
			"balance_transfer" => func!(balance_transfer),
			"balance_transfer_ea" => func!(balance_transfer_ea),
			"pay" => func!(pay),
			"contract_execute" => func!(contract_execute),
			"contract_execute_ea" => func!(contract_execute_ea),
		}
	};
	Ok(import_object)
}

fn share_read(ctx: &mut Ctx, share_id: u64, ptr: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);
	let value = state.share_to_vec(share_id)?;
	state.vec_to_memory(ptr, &value);
	Ok(())
}

fn share_len(ctx: &mut Ctx, share_id: u64) -> VMResult<u64> {
	let state = State::from_ctx(ctx);
	let len = state.share_to_len(share_id);
	Ok(len)
}

fn share_write(ctx: &mut Ctx, data_len: u64, data_ptr: u64, share_id: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);
	let data = state.memory_to_vec(data_len, data_ptr);
	state.vec_to_share(share_id, data)?;
	Ok(())
}

fn method_read(ctx: &mut Ctx, share_id: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);
	let data = state.method.as_bytes().to_vec();
	state.vec_to_share(share_id, data)?;
	Ok(())
}

fn params_read(ctx: &mut Ctx, share_id: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);
	let data = state.params.to_vec();
	state.vec_to_share(share_id, data)?;
	Ok(())
}

fn pay_value_read(ctx: &mut Ctx) -> VMResult<u64> {
	let state = State::from_ctx(ctx);
	Ok(state.pay_value)
}

fn result_write(ctx: &mut Ctx, len: u64, ptr: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);
	let result = state.memory_to_vec(len, ptr);
	state.result = Some(result);
	Ok(())
}

fn error_return(ctx: &mut Ctx, len: u64, ptr: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);
	let msg = state.memory_to_vec(len, ptr);
	let msg = String::from_utf8(msg).map_err(|_e| ContractError::BadUTF8)?;
	let error = ContractError::from(msg.as_str());
	Err(error.into())
}

/// for AssemblyScript
fn abort(_ctx: &mut Ctx, _msg_ptr: u32, _filename_ptr: u32, _line: u32, _col: u32) -> VMResult<()> {
	Err(VMError::Application(ApplicationError::ContractError(
		ContractError::Panic {
			msg: "AssemblyScript panic".to_string(),
		},
	)))
}

fn env_block_number(ctx: &mut Ctx) -> VMResult<u64> {
	let state = State::from_ctx(ctx);
	Ok(state.context.env().number)
}

fn env_block_timestamp(ctx: &mut Ctx) -> VMResult<u64> {
	let state = State::from_ctx(ctx);
	Ok(state.context.env().timestamp)
}

fn env_tx_hash_read(ctx: &mut Ctx, share_id: u64) -> VMResult<u64> {
	let state = State::from_ctx(ctx);
	let data = &state.context.call_env().tx_hash;
	match data.as_ref() {
		Some(data) => {
			state.vec_to_share(share_id, data.0.clone())?;
			Ok(1)
		}
		None => Ok(0),
	}
}

fn env_contract_address_read(ctx: &mut Ctx, share_id: u64) -> VMResult<u64> {
	let state = State::from_ctx(ctx);
	let data = &state.context.contract_env().contract_address;
	match data.as_ref() {
		Some(data) => {
			state.vec_to_share(share_id, data.0.clone())?;
			Ok(1)
		}
		None => Ok(0),
	}
}

fn env_sender_address_read(ctx: &mut Ctx, share_id: u64) -> VMResult<u64> {
	let state = State::from_ctx(ctx);
	let data = &state.context.contract_env().sender_address;
	match data.as_ref() {
		Some(data) => {
			state.vec_to_share(share_id, data.0.clone())?;
			Ok(1)
		}
		None => Ok(0),
	}
}

fn storage_read(ctx: &mut Ctx, key_len: u64, key_ptr: u64, share_id: u64) -> VMResult<u64> {
	let state = State::from_ctx(ctx);

	let key = state.memory_to_vec(key_len, key_ptr);

	let value = state.context.payload_get(&key)?;
	match value {
		Some(value) => {
			state.vec_to_share(share_id, value)?;
			Ok(1)
		}
		None => Ok(0),
	}
}

fn storage_write(
	ctx: &mut Ctx,
	key_len: u64,
	key_ptr: u64,
	value_exist: u64,
	value_len: u64,
	value_ptr: u64,
) -> VMResult<()> {
	let state = State::from_ctx(ctx);

	let key = state.memory_to_vec(key_len, key_ptr);

	let value = match value_exist {
		1 => {
			let value = state.memory_to_vec(value_len, value_ptr);
			Some(value)
		}
		_ => None,
	};

	state.context.payload_set(&key, value)?;

	Ok(())
}

fn event_write(ctx: &mut Ctx, len: u64, ptr: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);

	let event = state.memory_to_vec(len, ptr);
	state.context.emit_event(Event(event))?;
	Ok(())
}

fn util_hash(ctx: &mut Ctx, data_len: u64, data_ptr: u64, share_id: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);

	let data = state.memory_to_vec(data_len, data_ptr);
	let data = state.context.hash(&data)?.0;
	state.vec_to_share(share_id, data)?;
	Ok(())
}

fn util_address(ctx: &mut Ctx, data_len: u64, data_ptr: u64, share_id: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);

	let data = state.memory_to_vec(data_len, data_ptr);

	let data = state.context.address(&data)?.0;
	state.vec_to_share(share_id, data)?;
	Ok(())
}

fn util_validate_address(ctx: &mut Ctx, data_len: u64, data_ptr: u64) -> VMResult<()> {
	let state = State::from_ctx(ctx);

	let address = state.memory_to_vec(data_len, data_ptr);
	let address = Address(address);
	state.context.validate_address(&address)?;
	Ok(())
}

fn util_validate_address_ea(
	ctx: &mut Ctx,
	data_len: u64,
	data_ptr: u64,
	error_share_id: u64,
) -> VMResult<u64> {
	let result = util_validate_address(ctx, data_len, data_ptr);

	let state = State::from_ctx(ctx);

	state.make_error_aware(result, error_share_id)
}

fn balance_read(ctx: &mut Ctx, address_len: u64, address_ptr: u64) -> VMResult<u64> {
	let state = State::from_ctx(ctx);

	let address = state.memory_to_vec(address_len, address_ptr);
	let address = Address(address);

	let balance = state.context.module_balance_get(&address)?;

	Ok(balance)
}

fn balance_transfer(
	ctx: &mut Ctx,
	recipient_address_len: u64,
	recipient_address_ptr: u64,
	value: u64,
) -> VMResult<()> {
	let state = State::from_ctx(ctx);

	let sender_address = &state.context.contract_env().contract_address;
	let sender_address = sender_address
		.as_ref()
		.ok_or(ContractError::ContractAddressNotFound)?;

	let recipient_address = state.memory_to_vec(recipient_address_len, recipient_address_ptr);
	let recipient_address = Address(recipient_address);

	state
		.context
		.module_balance_transfer(sender_address, &recipient_address, value)?;

	state
		.context
		.module_payload_apply(state.context.module_payload_drain_buffer()?)?;
	state
		.context
		.module_apply_events(state.context.module_drain_events()?)?;

	Ok(())
}

fn balance_transfer_ea(
	ctx: &mut Ctx,
	recipient_address_len: u64,
	recipient_address_ptr: u64,
	value: u64,
	error_share_id: u64,
) -> VMResult<u64> {
	let result = balance_transfer(ctx, recipient_address_len, recipient_address_ptr, value);

	let state = State::from_ctx(ctx);

	if result.is_err() {
		state.context.module_payload_drain_buffer()?;
		state.context.module_drain_events()?;
	}
	state.make_error_aware(result, error_share_id)
}

fn pay(ctx: &mut Ctx) -> VMResult<()> {
	let state = State::from_ctx(ctx);

	let sender_address = &state.context.contract_env().sender_address;
	let sender_address = sender_address.as_ref().ok_or(ContractError::Unsigned)?;

	let contract_address = &state.context.contract_env().contract_address;
	let contract_address = contract_address
		.as_ref()
		.ok_or(ContractError::ContractAddressNotFound)?;
	let pay_value = state.pay_value;

	if pay_value > 0 {
		state
			.context
			.module_balance_transfer(sender_address, contract_address, pay_value)?;
	}

	state
		.context
		.module_payload_apply(state.context.module_payload_drain_buffer()?)?;
	state
		.context
		.module_apply_events(state.context.module_drain_events()?)?;

	Ok(())
}

fn contract_execute(
	ctx: &mut Ctx,
	contract_address_len: u64,
	contract_address_ptr: u64,
	method_len: u64,
	method_ptr: u64,
	params_len: u64,
	params_ptr: u64,
	pay_value: u64,
	share_id: u64,
) -> VMResult<()> {
	let state = State::from_ctx(ctx);
	let contract_address = state.memory_to_vec(contract_address_len, contract_address_ptr);
	let contract_address = Address(contract_address);
	let method = state.memory_to_vec(method_len, method_ptr);
	let method = String::from_utf8(method).map_err(|_| ContractError::InvalidMethod)?;
	let params = state.memory_to_vec(params_len, params_ptr);

	let result =
		state
			.context
			.nested_vm_contract_execute(&contract_address, &method, &params, pay_value)?;

	state.vec_to_share(share_id, result)?;

	state
		.context
		.nested_vm_payload_apply(state.context.nested_vm_payload_drain_buffer()?)?;
	state
		.context
		.nested_vm_apply_events(state.context.nested_vm_drain_events()?)?;

	Ok(())
}

fn contract_execute_ea(
	ctx: &mut Ctx,
	contract_address_len: u64,
	contract_address_ptr: u64,
	method_len: u64,
	method_ptr: u64,
	params_len: u64,
	params_ptr: u64,
	pay_value: u64,
	share_id: u64,
	error_share_id: u64,
) -> VMResult<u64> {
	let result = contract_execute(
		ctx,
		contract_address_len,
		contract_address_ptr,
		method_len,
		method_ptr,
		params_len,
		params_ptr,
		pay_value,
		share_id,
	);

	let state = State::from_ctx(ctx);

	if result.is_err() {
		state.context.nested_vm_payload_drain_buffer()?;
		state.context.nested_vm_drain_events()?;
	}
	state.make_error_aware(result, error_share_id)
}
