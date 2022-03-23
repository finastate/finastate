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

use std::error::Error;
use std::fmt::Debug;

use wasmer_runtime::error as wasmer_error;

pub use contract_sdk_primitives::ContractError;
use primitives::errors::{CommonError, CommonErrorKind, Display};

#[derive(Debug, Display)]
pub enum ErrorKind {
	#[display(fmt = "Wasm error: {}", _0)]
	Wasm(String),
}

impl Error for ErrorKind {}

impl From<ErrorKind> for CommonError {
	fn from(error: ErrorKind) -> Self {
		CommonError::new(CommonErrorKind::VM, Box::new(error))
	}
}

pub type VMResult<T> = Result<T, VMError>;

#[derive(Debug, Clone)]
pub enum VMError {
	/// System error, should not be accepted
	System(CommonError),
	/// Application error, should be accepted
	Application(ApplicationError),
}

#[derive(Debug, Clone, Display)]
pub enum ApplicationError {
	#[display(fmt = "PreCompileError: {}", _0)]
	PreCompileError(PreCompileError),
	#[display(fmt = "CompileError: {}", _0)]
	CompileError(CompileError),
	#[display(fmt = "LinkError: {}", msg)]
	LinkError { msg: String },
	#[display(fmt = "ResolveError: {}", _0)]
	ResolveError(ResolveError),
	#[display(fmt = "RuntimeError: {}", _0)]
	RuntimeError(RuntimeError),
	#[display(fmt = "ContractError: {}", _0)]
	ContractError(ContractError),
}

#[derive(Debug, Clone, Display)]
pub enum PreCompileError {
	#[display(fmt = "ValidationError: {}", msg)]
	ValidationError { msg: String },
	#[display(fmt = "Serialize")]
	Serialize,
	#[display(fmt = "Deserialize")]
	Deserialize,
	#[display(fmt = "InternalMemoryDeclared")]
	InternalMemoryDeclared,
	#[display(fmt = "StackHeightMetering")]
	StackHeightMetering,
	#[display(fmt = "Imports")]
	Imports,
}

#[derive(Debug, Clone, Display)]
pub enum CompileError {
	#[display(fmt = "ValidationError: {}", msg)]
	ValidationError { msg: String },
}

#[derive(Debug, Clone, Display)]
pub enum ResolveError {
	#[display(fmt = "Signature: {}", msg)]
	Signature { msg: String },
	#[display(fmt = "ExportNotFound: {}", name)]
	ExportNotFound { name: String },
	#[display(fmt = "ExportWrongType: {}", name)]
	ExportWrongType { name: String },
}

#[derive(Debug, Clone, Display)]
pub enum RuntimeError {
	#[display(fmt = "InvokeError: {}", _0)]
	InvokeError(InvokeError),
}

#[derive(Debug, Clone, Display)]
pub enum InvokeError {
	#[display(fmt = "FailedWithNoError")]
	FailedWithNoError,
	#[display(fmt = "Breakpoint")]
	Breakpoint,
}

impl From<wasmer_error::Error> for VMError {
	fn from(e: wasmer_error::Error) -> Self {
		match e {
			wasmer_error::Error::CompileError(e) => e.into(),
			wasmer_error::Error::LinkError(e) => e.into(),
			wasmer_error::Error::RuntimeError(e) => e.into(),
			wasmer_error::Error::ResolveError(e) => e.into(),
			wasmer_error::Error::CallError(e) => match e {
				wasmer_error::CallError::Runtime(e) => e.into(),
				wasmer_error::CallError::Resolve(e) => e.into(),
			},
			wasmer_error::Error::CreationError(e) => e.into(),
		}
	}
}

impl From<wasmer_error::CompileError> for VMError {
	fn from(e: wasmer_error::CompileError) -> Self {
		match e {
			wasmer_error::CompileError::InternalError { .. } => VMError::System(
				ErrorKind::Wasm(wasmer_error::Error::CompileError(e).to_string()).into(),
			),
			wasmer_error::CompileError::ValidationError { msg } => VMError::Application(
				ApplicationError::CompileError(CompileError::ValidationError { msg }),
			),
		}
	}
}

impl From<Vec<wasmer_error::LinkError>> for VMError {
	fn from(e: Vec<wasmer_error::LinkError>) -> Self {
		VMError::Application(ApplicationError::LinkError {
			msg: format!("{}", wasmer_error::Error::LinkError(e)),
		})
	}
}

impl From<wasmer_error::RuntimeError> for VMError {
	fn from(e: wasmer_error::RuntimeError) -> Self {
		match e {
			wasmer_error::RuntimeError::InvokeError(e) => match e {
				wasmer_error::InvokeError::FailedWithNoError => {
					VMError::Application(ApplicationError::RuntimeError(RuntimeError::InvokeError(
						InvokeError::FailedWithNoError,
					)))
				}
				wasmer_error::InvokeError::Breakpoint(_) => {
					VMError::Application(ApplicationError::RuntimeError(RuntimeError::InvokeError(
						InvokeError::Breakpoint,
					)))
				}
				_ => VMError::System(
					ErrorKind::Wasm(
						wasmer_error::Error::RuntimeError(wasmer_error::RuntimeError::InvokeError(
							e,
						))
						.to_string(),
					)
					.into(),
				),
			},
			wasmer_error::RuntimeError::User(e) => {
				if let Some(err) = e.downcast_ref::<VMError>() {
					err.clone()
				} else {
					VMError::System(
						ErrorKind::Wasm(
							wasmer_error::Error::RuntimeError(wasmer_error::RuntimeError::User(e))
								.to_string(),
						)
						.into(),
					)
				}
			}
			_ => VMError::System(
				ErrorKind::Wasm(wasmer_error::Error::RuntimeError(e).to_string()).into(),
			),
		}
	}
}

impl From<wasmer_error::ResolveError> for VMError {
	fn from(e: wasmer_error::ResolveError) -> Self {
		match e {
			wasmer_error::ResolveError::Signature { .. } => {
				VMError::Application(ApplicationError::ResolveError(ResolveError::Signature {
					msg: format!("{}", wasmer_error::Error::ResolveError(e)),
				}))
			}
			wasmer_error::ResolveError::ExportNotFound { name } => VMError::Application(
				ApplicationError::ResolveError(ResolveError::ExportNotFound { name }),
			),
			wasmer_error::ResolveError::ExportWrongType { name } => VMError::Application(
				ApplicationError::ResolveError(ResolveError::ExportWrongType { name }),
			),
		}
	}
}

impl From<wasmer_error::CreationError> for VMError {
	fn from(e: wasmer_error::CreationError) -> Self {
		VMError::System(ErrorKind::Wasm(wasmer_error::Error::CreationError(e).to_string()).into())
	}
}

impl From<wasmer_error::CallError> for VMError {
	fn from(e: wasmer_error::CallError) -> Self {
		match e {
			wasmer_error::CallError::Runtime(e) => e.into(),
			wasmer_error::CallError::Resolve(e) => e.into(),
		}
	}
}

impl From<PreCompileError> for VMError {
	fn from(e: PreCompileError) -> Self {
		VMError::Application(ApplicationError::PreCompileError(e))
	}
}

impl From<ContractError> for VMError {
	fn from(e: ContractError) -> Self {
		VMError::Application(ApplicationError::ContractError(e))
	}
}

impl From<CommonError> for VMError {
	fn from(e: CommonError) -> Self {
		VMError::System(e)
	}
}
