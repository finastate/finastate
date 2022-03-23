use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use sdk::{
	call, contract, import, init, serde_json, Address, Balance, BlockNumber, Context,
	ContractError, ContractResult, EmptyParams, Hash, Pay, StorageMap, StorageValue, Util,
};

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

struct Contract {
	context: Context,
	util: Util,
	pay: Pay,
	value: StorageValue<String>,
	map: StorageMap<String>,
}

#[contract]
impl Contract {
	fn new() -> ContractResult<Self> {
		let contract = Contract {
			context: Context::new()?,
			util: Util::new()?,
			pay: Pay::new(),
			value: StorageValue::new(b"value"),
			map: StorageMap::new(b"map"),
		};
		Ok(contract)
	}

	#[init]
	fn init(&self, params: InitParams) -> ContractResult<()> {
		self.value.set(&params.value)?;
		Ok(())
	}

	fn validate_hello(&self, params: HelloParams) -> ContractResult<()> {
		let name = params.name;
		if name.is_empty() {
			return Err(ContractError::User {
				msg: "Empty name".to_string(),
			});
		}
		Ok(())
	}

	#[call]
	fn hello(&self, params: HelloParams) -> ContractResult<String> {
		let name = params.name;
		let output = format!("hello {}", name);
		Ok(output)
	}

	#[call(payable = true)]
	fn pay_value(&self, _params: EmptyParams) -> ContractResult<Balance> {
		Ok(self.pay.pay_value())
	}

	#[call]
	fn error(&self, _params: EmptyParams) -> ContractResult<()> {
		Err(ContractError::User {
			msg: "Custom error".to_string(),
		})
	}

	#[call]
	fn get_env(&self, _params: EmptyParams) -> ContractResult<GetEnvOutput> {
		let env = self.context.env()?;
		let output = GetEnvOutput {
			number: env.number,
			timestamp: env.timestamp,
		};
		Ok(output)
	}

	#[call]
	fn get_call_env(&self, _params: EmptyParams) -> ContractResult<GetCallEnvOutput> {
		let call_env = self.context.call_env()?;
		let output = GetCallEnvOutput {
			tx_hash: call_env.tx_hash.clone(),
		};
		Ok(output)
	}

	#[call(payable = true)]
	fn get_contract_env(&self, _params: EmptyParams) -> ContractResult<GetContractEnvOutput> {
		let contract_env = self.context.contract_env()?;
		let output = GetContractEnvOutput {
			contract_address: contract_env.contract_address.clone(),
			sender_address: contract_env.sender_address.clone(),
		};
		Ok(output)
	}

	#[call]
	fn get_value(&self, _params: EmptyParams) -> ContractResult<GetValueOutput> {
		let value = self.value.get()?;
		let output = GetValueOutput { value };
		Ok(output)
	}

	#[call]
	fn set_value(&self, params: SetValueParams) -> ContractResult<()> {
		self.value.set(&params.value)?;
		Ok(())
	}

	#[call]
	fn delete_value(&self, _params: EmptyParams) -> ContractResult<()> {
		self.value.delete()?;
		Ok(())
	}

	#[call]
	fn get_map(&self, params: GetMapParams) -> ContractResult<GetMapOutput> {
		let key = params.key;
		let value = self.map.get(&key)?;
		let output = GetMapOutput { value };
		Ok(output)
	}

	#[call]
	fn set_map(&self, params: SetMapParams) -> ContractResult<()> {
		self.map.set(&params.key, &params.value)?;
		Ok(())
	}

	#[call]
	fn delete_map(&self, params: DeleteMapParams) -> ContractResult<()> {
		self.map.delete(&params.key)?;
		Ok(())
	}

	#[call]
	fn event(&self, _params: EmptyParams) -> ContractResult<()> {
		self.context.emit_event(
			"MyEvent".to_string(),
			MyEvent {
				foo: "bar".to_string(),
			},
		)?;
		Ok(())
	}

	#[call]
	fn get_balance(&self, params: GetBalanceParams) -> ContractResult<Balance> {
		let address = params.address;
		let balance = self.context.balance_get(&address)?;
		Ok(balance)
	}

	#[call(payable = true)]
	fn balance_transfer(&self, params: BalanceTransferParams) -> ContractResult<()> {
		self.context
			.balance_transfer(&params.recipient, params.value)?;
		Ok(())
	}

	#[call(payable = true)]
	fn balance_transfer_ea(&self, params: BalanceTransferParams) -> ContractResult<String> {
		let result = self
			.context
			.balance_transfer_ea(&params.recipient, params.value);
		let result = match result {
			Ok(_) => "true".to_string(),
			Err(e) => format!("false: {}", e),
		};
		Ok(result)
	}

	#[call]
	fn hash(&self, params: ComputeHashParams) -> ContractResult<Hash> {
		let result = self.util.hash(&params.data)?;
		Ok(result)
	}

	#[call]
	fn address(&self, params: ComputeAddressParams) -> ContractResult<Address> {
		let result = self.util.address(&params.data)?;
		Ok(result)
	}

	#[call]
	fn verify_address(&self, params: VerifyAddressParams) -> ContractResult<()> {
		self.util.validate_address(&params.address)?;
		Ok(())
	}

	#[call]
	fn verify_address_ea(&self, params: VerifyAddressParams) -> ContractResult<String> {
		let result = self.util.validate_address_ea(&params.address);
		let result = match result {
			Ok(_) => "true".to_string(),
			Err(e) => format!("false: {}", e),
		};
		Ok(result)
	}

	#[call]
	fn nested_contract_execute(&self, _params: EmptyParams) -> ContractResult<Vec<u8>> {
		let contract_address = &self.context.contract_env()?.contract_address;
		let contract_address = contract_address
			.as_ref()
			.ok_or(ContractError::ContractAddressNotFound)?;
		let method = "nested_contract_execute";
		let params = "".as_bytes();
		let pay_value = 0;
		let result = self
			.context
			.contract_execute(contract_address, method, params, pay_value)?;
		Ok(result)
	}
}

#[derive(Deserialize)]
struct InitParams {
	pub value: String,
}

#[derive(Deserialize)]
struct HelloParams {
	name: String,
}

#[derive(Serialize)]
struct GetEnvOutput {
	pub number: BlockNumber,
	pub timestamp: u64,
}

#[derive(Serialize)]
struct GetCallEnvOutput {
	pub tx_hash: Option<Hash>,
}

#[derive(Serialize)]
struct GetContractEnvOutput {
	pub contract_address: Option<Address>,
	pub sender_address: Option<Address>,
}

#[derive(Serialize)]
struct GetValueOutput {
	pub value: Option<String>,
}

#[derive(Deserialize)]
struct SetValueParams {
	pub value: String,
}

#[derive(Deserialize)]
struct GetMapParams {
	pub key: Vec<u8>,
}

#[derive(Serialize)]
struct GetMapOutput {
	pub value: Option<String>,
}

#[derive(Deserialize)]
struct SetMapParams {
	pub key: Vec<u8>,
	pub value: String,
}

#[derive(Deserialize)]
struct DeleteMapParams {
	pub key: Vec<u8>,
}

#[derive(Serialize)]
pub struct MyEvent {
	foo: String,
}

#[derive(Deserialize)]
struct GetBalanceParams {
	pub address: Address,
}

#[derive(Deserialize)]
struct BalanceTransferParams {
	recipient: Address,
	value: Balance,
}

#[derive(Deserialize)]
struct ComputeHashParams {
	data: Vec<u8>,
}

#[derive(Deserialize)]
struct ComputeAddressParams {
	data: Vec<u8>,
}

#[derive(Deserialize)]
struct VerifyAddressParams {
	address: Address,
}
