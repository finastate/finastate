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

use derive_more::{Display, From, TryInto};
use primitives::codec::{Decode, Encode};
use primitives::{Address, BlockNumber, Hash, PublicKey, Signature, Transaction};
use utils_enum_codec::enum_codec;

#[enum_codec]
#[derive(From, TryInto)]
pub enum HotStuffMessage {
	RegisterValidatorReq(RegisterValidatorReq),
	RegisterValidatorRes(RegisterValidatorRes),
	RequestProposalReq(RequestProposalReq),
	RequestProposalRes(RequestProposalRes),
	ConsensusMessage(ConsensusMessage),
	TimeoutMessage(TimeoutMessage),
}

#[derive(Encode, Decode, Debug)]
pub struct RegisterValidatorReq {
	pub request_id: RequestId,
	pub public_key: PublicKey,
	pub signature: Signature,
}

#[derive(Encode, Decode, Debug)]
pub struct RegisterValidatorRes {
	pub request_id: RequestId,
	pub success: bool,
}

#[derive(Encode, Decode, Debug)]
pub struct RequestProposalReq {
	pub request_id: RequestId,
	pub block_hash: Hash,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct RequestProposalRes {
	pub request_id: RequestId,
	pub proposal: Option<Proposal>,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct ConsensusMessage {
	pub request_id: RequestId,
	pub message_type: MessageType,
	pub view: u64,
	pub node: Option<Node>,
	pub justify: Option<QC>,
	pub sig: Option<(PublicKey, Signature)>,
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub enum MessageType {
	NewView,
	Prepare,
	PreCommit,
	Commit,
	Decide,
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub struct QC {
	pub message_type: MessageType,
	pub view: u64,
	pub node: Node,
	pub leader_address: Address,
	pub sig: Vec<(PublicKey, Signature)>,
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub struct Node {
	pub parent_hash: Hash,
	pub block_hash: Hash,
}

#[derive(Encode, Decode, Clone, Debug)]
pub struct Proposal {
	pub parent_hash: Hash,
	pub block_hash: Hash,
	pub number: BlockNumber,
	pub timestamp: u64,
	pub meta_txs: Vec<Transaction>,
	pub payload_txs: Vec<Transaction>,
	pub execution_number: BlockNumber,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct TimeoutMessage {
	pub request_id: RequestId,
	pub message_type: TimeoutMessageType,
	pub view: u64,
	pub justify: Option<TC>,
	pub sig: Option<(PublicKey, Signature)>,
}

#[derive(Encode, Decode, Debug, Clone, PartialEq)]
pub enum TimeoutMessageType {
	NewTimeout,
	Decide,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct TC {
	pub view: u64,
	pub sig: Vec<(PublicKey, Signature)>,
}

impl RequestIdAware for RegisterValidatorReq {
	fn get_request_id(&self) -> RequestId {
		self.request_id.clone()
	}
	fn set_request_id(&mut self, request_id: RequestId) {
		self.request_id = request_id;
	}
}

impl RequestIdAware for RegisterValidatorRes {
	fn get_request_id(&self) -> RequestId {
		self.request_id.clone()
	}
	fn set_request_id(&mut self, request_id: RequestId) {
		self.request_id = request_id;
	}
}

impl RequestIdAware for RequestProposalReq {
	fn get_request_id(&self) -> RequestId {
		self.request_id.clone()
	}
	fn set_request_id(&mut self, request_id: RequestId) {
		self.request_id = request_id;
	}
}

impl RequestIdAware for RequestProposalRes {
	fn get_request_id(&self) -> RequestId {
		self.request_id.clone()
	}
	fn set_request_id(&mut self, request_id: RequestId) {
		self.request_id = request_id;
	}
}

impl RequestIdAware for ConsensusMessage {
	fn get_request_id(&self) -> RequestId {
		self.request_id.clone()
	}
	fn set_request_id(&mut self, request_id: RequestId) {
		self.request_id = request_id;
	}
}

impl RequestIdAware for TimeoutMessage {
	fn get_request_id(&self) -> RequestId {
		self.request_id.clone()
	}
	fn set_request_id(&mut self, request_id: RequestId) {
		self.request_id = request_id;
	}
}

pub trait RequestIdAware {
	fn get_request_id(&self) -> RequestId;
	fn set_request_id(&mut self, request_id: RequestId);
}

#[derive(Encode, Decode, Debug, PartialEq, Clone, Display, Hash, Eq)]
pub struct RequestId(pub u64);
