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

use std::sync::Arc;

use executor_macro::{call, module};
use executor_primitives::{
	errors, Context, ContextEnv, EmptyParams, Module as ModuleT, ModuleResult, OpaqueModuleResult,
	StorageValue, Util,
};
use node_consensus_primitives::CONSENSUS_LIST;
use primitives::codec::{Decode, Encode};
use primitives::types::ExecutionGap;
use primitives::{codec, Address, BlockNumber, Call};

pub struct Module<C, U>
where
	C: Context,
	U: Util,
{
	env: Arc<ContextEnv>,
	#[allow(dead_code)]
	util: U,
	chain_id: StorageValue<String, Self>,
	timestamp: StorageValue<u64, Self>,
	max_until_gap: StorageValue<BlockNumber, Self>,
	max_execution_gap: StorageValue<ExecutionGap, Self>,
	consensus: StorageValue<String, Self>,
}

#[module]
impl<C: Context, U: Util> Module<C, U> {
	const META_MODULE: bool = true;
	const STORAGE_KEY: &'static [u8] = b"system";

	fn new(context: C, util: U) -> Self {
		Self {
			env: context.env(),
			util,
			chain_id: StorageValue::new(context.clone(), b"chain_id"),
			timestamp: StorageValue::new(context.clone(), b"timestamp"),
			max_until_gap: StorageValue::new(context.clone(), b"max_until_gap"),
			max_execution_gap: StorageValue::new(context.clone(), b"max_execution_gap"),
			consensus: StorageValue::new(context, b"consensus"),
		}
	}

	#[call(write = true)]
	fn init(&self, _sender: Option<&Address>, params: InitParams) -> ModuleResult<()> {
		if self.env.number != 0 {
			return Err("Not genesis".into());
		}
		self.chain_id.set(&params.chain_id)?;
		self.timestamp.set(&params.timestamp)?;
		self.max_until_gap.set(&params.max_until_gap)?;
		self.max_execution_gap.set(&params.max_execution_gap)?;
		self.consensus.set(&params.consensus)?;
		Ok(())
	}

	fn validate_init(&self, _sender: Option<&Address>, params: InitParams) -> ModuleResult<()> {
		if !CONSENSUS_LIST.iter().any(|&x| x == params.consensus) {
			return Err("Unknown consensus".into());
		}
		Ok(())
	}

	#[call]
	fn get_meta(&self, _sender: Option<&Address>, _params: EmptyParams) -> ModuleResult<Meta> {
		let chain_id = self.chain_id.get()?.ok_or("Unexpected none")?;
		let timestamp = self.timestamp.get()?.ok_or("Unexpected none")?;
		let max_until_gap = self.max_until_gap.get()?.ok_or("Unexpected none")?;
		let max_execution_gap = self.max_execution_gap.get()?.ok_or("Unexpected none")?;
		let consensus = self.consensus.get()?.ok_or("Unexpected none")?;
		let meta = Meta {
			chain_id,
			timestamp,
			max_until_gap,
			max_execution_gap,
			consensus,
		};
		Ok(meta)
	}
}

pub type InitParams = Meta;

#[derive(Encode, Decode, Debug, PartialEq)]
pub struct Meta {
	pub chain_id: String,
	pub timestamp: u64,
	pub max_until_gap: BlockNumber,
	pub max_execution_gap: ExecutionGap,
	pub consensus: String,
}
