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
use std::sync::Arc;

use parking_lot::RwLock;

use node_chain::DBTransaction;
use node_consensus_base::support::ConsensusSupport;
use primitives::errors::CommonResult;
use primitives::{codec, Hash};

use crate::get_hotstuff_authorities;
use crate::proof::Proof;
use crate::protocol::{MessageType, Node, Proposal, QC};

const DB_KEY_VIEW: &[u8] = b"view";
const DB_KEY_PREPARE_QC: &[u8] = b"prepare_qc";
const DB_KEY_LOCKED_QC: &[u8] = b"locked_qc";
const DB_KEY_NODES: &[u8] = b"nodes";
const DB_KEY_PROPOSALS: &[u8] = b"proposals";
const DB_KEY_PROPOSAL_PREFIX: &[u8] = b"proposal_";

pub struct Storage<S>
where
	S: ConsensusSupport,
{
	base_commit_qc: RwLock<QC>,
	base_block_hash: RwLock<Hash>,
	view: RwLock<u64>,
	prepare_qc: RwLock<QC>,
	locked_qc: RwLock<QC>,
	nodes: RwLock<HashMap<Hash, Node>>,
	proposals: RwLock<HashMap<Hash, Node>>,
	support: Arc<S>,
}

impl<S> Storage<S>
where
	S: ConsensusSupport,
{
	pub fn new(support: Arc<S>) -> CommonResult<Self> {
		let genesis_qc = Self::get_genesis_qc(&support)?;

		let this = Self {
			base_commit_qc: RwLock::new(genesis_qc.clone()),
			base_block_hash: RwLock::new(Hash(Default::default())),
			view: RwLock::new(0),
			prepare_qc: RwLock::new(genesis_qc.clone()),
			locked_qc: RwLock::new(genesis_qc),
			nodes: RwLock::new(HashMap::new()),
			proposals: RwLock::new(HashMap::new()),
			support,
		};
		this.refresh()?;
		Ok(this)
	}

	pub fn refresh(&self) -> CommonResult<()> {
		let proof = self.get_proof()?;
		let genesis_qc = Self::get_genesis_qc(&self.support)?;

		let base_commit_qc = match proof {
			Some(proof) => proof.commit_qc,
			None => genesis_qc.clone(),
		};

		let current_state = self.support.get_current_state();
		let base_block_hash = current_state.confirmed_block_hash.clone();

		// init view
		// and fix if needed
		let mut view: u64 = self
			.support
			.get_consensus_data(DB_KEY_VIEW)?
			.unwrap_or_default();

		if view < base_commit_qc.view + 1 {
			view = base_commit_qc.view + 1;
			self.commit_consensus_data(|transaction| {
				self.support
					.put_consensus_data(transaction, DB_KEY_VIEW, view)?;
				Ok(())
			})?;
		}

		// init prepare qc
		// and fix if needed
		let mut prepare_qc: QC = self
			.support
			.get_consensus_data(DB_KEY_PREPARE_QC)?
			.unwrap_or_else(|| genesis_qc.clone());

		if prepare_qc.view <= base_commit_qc.view {
			prepare_qc = base_commit_qc.clone();
			self.commit_consensus_data(|transaction| {
				self.support
					.put_consensus_data(transaction, DB_KEY_PREPARE_QC, &prepare_qc)?;
				Ok(())
			})?;
		}

		// init locked qc
		// and fix if needed
		let mut locked_qc: QC = self
			.support
			.get_consensus_data(DB_KEY_LOCKED_QC)?
			.unwrap_or(genesis_qc);

		if locked_qc.view <= base_commit_qc.view {
			locked_qc = base_commit_qc.clone();
			self.commit_consensus_data(|transaction| {
				self.support
					.put_consensus_data(transaction, DB_KEY_LOCKED_QC, &locked_qc)?;
				Ok(())
			})?;
		}

		// init nodes
		// and fix if needed
		let mut nodes = {
			let nodes: Vec<(Hash, Node)> = self
				.support
				.get_consensus_data(DB_KEY_NODES)?
				.unwrap_or_default();
			nodes.into_iter().collect::<HashMap<_, _>>()
		};

		let to_remove_key = nodes
			.iter()
			.filter(|(_, v)| v.parent_hash != base_block_hash)
			.map(|(k, _)| k.clone())
			.collect::<Vec<_>>();
		for k in &to_remove_key {
			nodes.remove(k);
		}
		if !to_remove_key.is_empty() {
			let nodes_vec = nodes.iter().collect::<Vec<_>>();
			self.commit_consensus_data(|transaction| {
				self.support
					.put_consensus_data(transaction, DB_KEY_NODES, &nodes_vec)?;
				Ok(())
			})?;
		}

		// init proposals
		// and fix if needed
		let mut proposals = {
			let proposals: Vec<(Hash, Node)> = self
				.support
				.get_consensus_data(DB_KEY_PROPOSALS)?
				.unwrap_or_default();
			proposals.into_iter().collect::<HashMap<_, _>>()
		};

		let to_remove_key = proposals
			.iter()
			.filter(|(_, v)| v.parent_hash != base_block_hash)
			.map(|(k, _)| k.clone())
			.collect::<Vec<_>>();
		for k in &to_remove_key {
			let proposal_key = [DB_KEY_PROPOSAL_PREFIX, &k.0].concat();
			self.commit_consensus_data(|transaction| {
				self.support
					.delete_consensus_data(transaction, &proposal_key)?;
				Ok(())
			})?;
			proposals.remove(k);
		}
		if !to_remove_key.is_empty() {
			let nodes_vec = nodes.iter().collect::<Vec<_>>();
			self.commit_consensus_data(|transaction| {
				self.support
					.put_consensus_data(transaction, DB_KEY_NODES, &nodes_vec)?;
				Ok(())
			})?;
		}

		(*self.base_commit_qc.write()) = base_commit_qc;
		(*self.base_block_hash.write()) = base_block_hash;
		(*self.view.write()) = view;
		(*self.prepare_qc.write()) = prepare_qc;
		(*self.locked_qc.write()) = locked_qc;
		(*self.nodes.write()) = nodes;
		(*self.proposals.write()) = proposals;

		Ok(())
	}

	pub fn get_base_commit_qc(&self) -> QC {
		self.base_commit_qc.read().clone()
	}

	pub fn get_view(&self) -> u64 {
		*self.view.read()
	}

	pub fn update_view(&self, view: u64) -> CommonResult<()> {
		self.commit_consensus_data(|transaction| {
			self.support
				.put_consensus_data(transaction, DB_KEY_VIEW, view)?;
			Ok(())
		})?;
		*self.view.write() = view;
		Ok(())
	}

	pub fn get_prepare_qc(&self) -> QC {
		self.prepare_qc.read().clone()
	}

	pub fn update_prepare_qc(&self, prepare_qc: QC) -> CommonResult<()> {
		self.commit_consensus_data(|transaction| {
			self.support
				.put_consensus_data(transaction, DB_KEY_PREPARE_QC, prepare_qc.clone())?;
			Ok(())
		})?;
		*self.prepare_qc.write() = prepare_qc;
		Ok(())
	}

	pub fn get_locked_qc(&self) -> QC {
		self.locked_qc.read().clone()
	}

	pub fn update_locked_qc(&self, locked_qc: QC) -> CommonResult<()> {
		self.commit_consensus_data(|transaction| {
			self.support
				.put_consensus_data(transaction, DB_KEY_LOCKED_QC, locked_qc.clone())?;
			Ok(())
		})?;
		*self.locked_qc.write() = locked_qc;
		Ok(())
	}

	pub fn get_proposal(&self, block_hash: &Hash) -> CommonResult<Option<Proposal>> {
		self.get_proposal_using(block_hash, |x| x.clone())
	}

	pub fn get_proposal_using<T, F: Fn(&Option<Proposal>) -> T>(
		&self,
		block_hash: &Hash,
		using: F,
	) -> CommonResult<T> {
		let proposal_key = [DB_KEY_PROPOSAL_PREFIX, &block_hash.0].concat();

		let proposal: Option<Proposal> = self.support.get_consensus_data(&proposal_key)?;

		Ok(using(&proposal))
	}

	pub fn put_proposal(&self, proposal: Proposal) -> CommonResult<()> {
		let proposal_key = [DB_KEY_PROPOSAL_PREFIX, &proposal.block_hash.0].concat();

		// save proposals
		let node = Node {
			block_hash: proposal.block_hash.clone(),
			parent_hash: proposal.parent_hash.clone(),
		};
		(*self.proposals.write()).insert(node.block_hash.clone(), node);
		let proposals_vec = self
			.proposals
			.read()
			.iter()
			.map(|(k, v)| (k.clone(), v.clone()))
			.collect::<Vec<_>>();
		self.commit_consensus_data(|transaction| {
			self.support
				.put_consensus_data(transaction, DB_KEY_PROPOSALS, &proposals_vec)?;
			Ok(())
		})?;

		// save proposal
		self.commit_consensus_data(|transaction| {
			self.support
				.put_consensus_data(transaction, &proposal_key, &proposal)?;
			Ok(())
		})?;
		Ok(())
	}

	fn get_genesis_qc(support: &Arc<S>) -> CommonResult<QC> {
		let genesis_hash = support.get_current_state().genesis_hash.clone();
		let genesis_authorities = get_hotstuff_authorities(&support, &0)?;
		let genesis_leader_address = genesis_authorities.members[0].0.clone();
		Ok(QC {
			message_type: MessageType::Decide,
			view: 0,
			node: Node {
				block_hash: genesis_hash,
				parent_hash: Hash(vec![]),
			},
			leader_address: genesis_leader_address,
			sig: vec![],
		})
	}

	fn get_proof(&self) -> CommonResult<Option<Proof>> {
		let current_state = self.support.get_current_state();
		let confirmed_number = current_state.confirmed_number;
		let proof = match confirmed_number {
			0 => None,
			_ => {
				let confirmed_block_hash = &current_state.confirmed_block_hash;
				let proof = self
					.support
					.get_proof(confirmed_block_hash)?
					.ok_or_else(|| {
						node_consensus_base::errors::ErrorKind::Data(format!(
							"Missing proof: block_hash: {}",
							confirmed_block_hash
						))
					})?;
				let data = proof.data;
				let proof: Proof = codec::decode(&mut &data[..]).map_err(|_| {
					node_consensus_base::errors::ErrorKind::Data("Decode proof error".to_string())
				})?;
				Some(proof)
			}
		};
		Ok(proof)
	}

	fn commit_consensus_data<OP: Fn(&mut DBTransaction) -> CommonResult<()>>(
		&self,
		op: OP,
	) -> CommonResult<()> {
		let mut transaction = DBTransaction::new();
		op(&mut transaction)?;
		self.support.commit_consensus_data(transaction)
	}
}
