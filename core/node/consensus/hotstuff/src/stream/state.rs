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

use crate::errors::ErrorKind;
use crate::protocol::{
	ConsensusMessage, MessageType, Node, Proposal, RequestId, RequestProposalReq,
	RequestProposalRes, QC,
};
use crate::stream::{get_address, sign, verify, verify_qc, HotStuffStream, InternalMessage};
use futures::prelude::*;
use log::{trace, warn};
use node_consensus_base::scheduler::{ScheduleInfo, Scheduler};
use node_consensus_base::support::ConsensusSupport;
use node_consensus_base::PeerId;
use node_executor::module::hotstuff::Authorities;
use primitives::errors::CommonResult;
use primitives::{codec, BuildBlockParams, FullTransaction, Transaction};
use primitives::{Address, Hash};
use rand::{thread_rng, Rng};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::sleep_until;

#[derive(PartialEq, Debug)]
pub enum State {
	Leader,
	Replica,
	Observer,
}

pub struct LeaderState<'a, S>
where
	S: ConsensusSupport,
{
	stream: &'a mut HotStuffStream<S>,
	new_view_collector: ConsensusMessageCollector,
	prepare_collector: ConsensusMessageCollector,
	pre_commit_collector: ConsensusMessageCollector,
	commit_collector: ConsensusMessageCollector,
	work_schedule_signal: Option<ScheduleInfo>,
	work_new_view_signal: Option<QC>,
	pending_remote_proposal: Option<Hash>,
	prepare_message: Option<ConsensusMessage>,
	pre_commit_message: Option<ConsensusMessage>,
	commit_message: Option<ConsensusMessage>,
	decide_message: Option<ConsensusMessage>,
}

impl<'a, S> LeaderState<'a, S>
where
	S: ConsensusSupport,
{
	pub fn new(stream: &'a mut HotStuffStream<S>) -> Self {
		Self {
			stream,
			new_view_collector: ConsensusMessageCollector::new(),
			prepare_collector: ConsensusMessageCollector::new(),
			pre_commit_collector: ConsensusMessageCollector::new(),
			commit_collector: ConsensusMessageCollector::new(),
			work_schedule_signal: None,
			work_new_view_signal: None,
			pending_remote_proposal: None,
			prepare_message: None,
			pre_commit_message: None,
			commit_message: None,
			decide_message: None,
		}
	}
	pub async fn start(mut self) -> CommonResult<()> {
		let current_view = self.stream.storage.get_view();
		self.process_pending_new_view()?;
		self.send_new_view_to_self()?;

		let mut scheduler = Scheduler::new(self.stream.hotstuff_meta.block_interval);

		loop {
			if self.stream.shutdown {
				return Ok(());
			}
			if self.stream.storage.get_view() != current_view {
				return Ok(());
			}
			let next_timeout = sleep_until(self.stream.next_timeout_instant());

			tokio::select! {
				_ = next_timeout => {
					self.stream.on_timeout()?;
				},
				Some(schedule_info) = scheduler.next() => {
					self.try_work(Some(schedule_info), None)
					.unwrap_or_else(|e| warn!("HotStuff stream handle work error: {}", e));
				}
				Some(internal_message) = self.stream.internal_rx.next() => {
					match internal_message {
						InternalMessage::ValidatorRegistered { address, peer_id } => {
							self.on_validator_registered(address, peer_id)
								.unwrap_or_else(|e| warn!("HotStuff stream handle validator registered message error: {}", e));
						},
						InternalMessage::Proposal { address, proposal } => {
							self.on_remote_proposal(address, proposal)
								.unwrap_or_else(|e| warn!("HotStuff stream handle proposal message error: {}", e));
						},
						InternalMessage::ConsensusMessage {message, peer_id } => {
							self.on_consensus_message(message, peer_id)
								.unwrap_or_else(|e| warn!("HotStuff stream handle consensus message error: {}", e));
						}
					}
				},
				in_message = self.stream.in_rx.next() => {
					match in_message {
						Some(in_message) => {
							self.stream.on_in_message(in_message)
								.unwrap_or_else(|e| warn!("HotStuff stream handle in message error: {}", e));
						},
						// in tx has been dropped
						None => {
							self.stream.shutdown = true;
						},
					}
				},
			}
		}
	}

	fn try_work(
		&mut self,
		work_schedule_signal: Option<ScheduleInfo>,
		work_new_view_signal: Option<QC>,
	) -> CommonResult<()> {
		if let Some(work_schedule_signal) = work_schedule_signal {
			self.work_schedule_signal = Some(work_schedule_signal);
		}
		if let Some(work_new_view_signal) = work_new_view_signal {
			self.work_new_view_signal = Some(work_new_view_signal);
		}
		if self.work_new_view_signal.is_some()
			&& self.work_schedule_signal.is_some()
			&& self.prepare_message.is_none()
			&& self.pending_remote_proposal.is_none()
		{
			self.work()?;
		}
		Ok(())
	}

	fn work(&mut self) -> CommonResult<()> {
		let high_qc = self.work_new_view_signal.clone().expect("qed");
		let base_commit_qc = self.stream.storage.get_base_commit_qc();

		if high_qc.node.block_hash == base_commit_qc.node.block_hash {
			let schedule_info = self.work_schedule_signal.clone().expect("qed");
			self.create_proposal(schedule_info)?;
		} else if high_qc.node.parent_hash == base_commit_qc.node.block_hash {
			self.reuse_proposal(&high_qc)?;
		}

		Ok(())
	}

	fn create_proposal(&mut self, schedule_info: ScheduleInfo) -> CommonResult<()> {
		trace!("Create proposal: {:?}", schedule_info);
		let build_block_params = self.stream.support.prepare_block(schedule_info)?;

		let mut proposal = Proposal {
			parent_hash: Hash(vec![]),
			block_hash: Hash(vec![]),
			number: build_block_params.number,
			timestamp: build_block_params.timestamp,
			meta_txs: build_block_params
				.meta_txs
				.iter()
				.map(|x| x.tx.clone())
				.collect(),
			payload_txs: build_block_params
				.payload_txs
				.iter()
				.map(|x| x.tx.clone())
				.collect(),
			execution_number: build_block_params.execution_number,
		};

		let commit_block_params = self.stream.support.build_block(build_block_params)?;
		proposal.parent_hash = commit_block_params.header.parent_hash.clone();
		proposal.block_hash = commit_block_params.block_hash.clone();

		self.stream.commit_block_params = Some(commit_block_params);
		self.stream.storage.put_proposal(proposal.clone())?;

		self.on_proposal(proposal)?;
		Ok(())
	}

	fn reuse_proposal(&mut self, high_qc: &QC) -> CommonResult<()> {
		trace!("Reuse proposal: {:?}", high_qc);
		let proposal = self.stream.storage.get_proposal(&high_qc.node.block_hash)?;
		match proposal {
			Some(proposal) => {
				self.on_local_proposal(proposal)?;
			}
			None => {
				let addresses = self.new_view_collector.get_high_qc_addresses(high_qc);
				let remote_addresses = addresses
					.iter()
					.filter(|x| *x != &self.stream.address)
					.collect::<Vec<_>>();
				let rand = thread_rng().gen_range(0, remote_addresses.len());

				let target_address = remote_addresses[rand];
				trace!("Request proposal from remote: {}", target_address);

				let req = RequestProposalReq {
					request_id: RequestId(0),
					block_hash: high_qc.node.block_hash.clone(),
				};
				self.stream.request_proposal(target_address.clone(), req)?;
				self.pending_remote_proposal = Some(high_qc.node.block_hash.clone());
			}
		}
		Ok(())
	}

	fn on_remote_proposal(&mut self, _address: Address, proposal: Proposal) -> CommonResult<()> {
		if let Some(pending_remote_proposal) = self.pending_remote_proposal.take() {
			if pending_remote_proposal != proposal.block_hash {
				self.pending_remote_proposal = Some(pending_remote_proposal);
				return Ok(());
			}
			self.on_local_proposal(proposal)?;
		}
		Ok(())
	}

	fn on_local_proposal(&mut self, proposal: Proposal) -> CommonResult<()> {
		// prepare commit_block_params if needed
		if self
			.stream
			.commit_block_params
			.as_ref()
			.map(|x| &x.block_hash)
			!= Some(&proposal.block_hash)
		{
			let get_txs = |txs: &Vec<Transaction>| -> CommonResult<Vec<Arc<FullTransaction>>> {
				let mut result = Vec::with_capacity(txs.len());
				for tx in txs {
					let tx_hash = self.stream.support.hash_transaction(&tx)?;
					let tx = Arc::new(FullTransaction {
						tx_hash,
						tx: tx.clone(),
					});
					result.push(tx);
				}
				Ok(result)
			};

			let build_block_params = BuildBlockParams {
				number: proposal.number,
				timestamp: proposal.timestamp,
				meta_txs: get_txs(&proposal.meta_txs)?,
				payload_txs: get_txs(&proposal.payload_txs)?,
				execution_number: proposal.execution_number,
			};
			let commit_block_params = self.stream.support.build_block(build_block_params)?;
			self.stream.commit_block_params = Some(commit_block_params);
		}
		self.on_proposal(proposal)?;
		Ok(())
	}

	fn on_validator_registered(&mut self, address: Address, peer_id: PeerId) -> CommonResult<()> {
		if let Some(messages) = self.stream.pending_consensus_messages.remove(&peer_id) {
			for message in messages {
				self.on_validator_consensus_message(message, address.clone())?;
			}
		}
		Ok(())
	}

	fn on_consensus_message(
		&mut self,
		message: ConsensusMessage,
		peer_id: PeerId,
	) -> CommonResult<()> {
		if let Some(address) = self.stream.get_peer_address(&peer_id) {
			if self
				.stream
				.authorities
				.members
				.iter()
				.any(|(addr, _)| addr == &address)
			{
				self.on_validator_consensus_message(message, address)?;
			}
		} else {
			let messages = self
				.stream
				.pending_consensus_messages
				.entry(peer_id)
				.or_insert_with(Vec::new);
			messages.push(message);
		}
		Ok(())
	}

	fn on_validator_consensus_message(
		&mut self,
		message: ConsensusMessage,
		address: Address,
	) -> CommonResult<()> {
		if message.view != self.stream.storage.get_view() {
			return Ok(());
		}
		match &message.message_type {
			MessageType::NewView => self.on_new_view(message, address)?,
			MessageType::Prepare => self.on_prepare(message, address)?,
			MessageType::PreCommit => self.on_pre_commit(message, address)?,
			MessageType::Commit => self.on_commit(message, address)?,
			_ => {}
		}
		Ok(())
	}

	fn on_new_view(&mut self, message: ConsensusMessage, address: Address) -> CommonResult<()> {
		let justify = match &message.justify {
			Some(justify) => justify,
			None => return Ok(()),
		};

		let authorities = if justify.view == self.stream.storage.get_base_commit_qc().view {
			&self.stream.last_authorities
		} else {
			&self.stream.authorities
		};

		match verify_qc(&justify, None, &authorities, &self.stream.support) {
			Err(e) => {
				println!(
					"on_new_view verify qc failed: {}, view: {}, base_commit_qc_view: {}",
					e,
					justify.view,
					self.stream.storage.get_base_commit_qc().view
				);
				return Ok(());
			}
			_ => {}
		}

		self.new_view_collector.add(address, message);

		let mut high_qc = self
			.new_view_collector
			.get_high_qc(&self.stream.authorities);

		if let Some(high_qc) = high_qc.take() {
			if self.work_new_view_signal.is_none() {
				trace!("New view collected high qc: {:?}", high_qc);
				self.try_work(None, Some(high_qc))?;
			}
		}
		Ok(())
	}

	fn on_proposal(&mut self, proposal: Proposal) -> CommonResult<()> {
		trace!(
			"Proposal: number: {}, execution_number: {}, block_hash: {}",
			proposal.number,
			proposal.execution_number,
			proposal.block_hash
		);

		let node = Node {
			block_hash: proposal.block_hash.clone(),
			parent_hash: proposal.parent_hash.clone(),
		};

		let high_qc = self.work_new_view_signal.clone().expect("qed");

		let prepare_message = ConsensusMessage {
			request_id: RequestId(0),
			message_type: MessageType::Prepare,
			view: self.stream.storage.get_view(),
			node: Some(node),
			justify: Some(high_qc),
			sig: None,
		};
		self.prepare_message = Some(prepare_message.clone());
		self.on_prepare(prepare_message.clone(), self.stream.address.clone())?;

		let addresses = self
			.stream
			.authorities
			.members
			.iter()
			.map(|x| x.0.clone())
			.collect::<Vec<_>>();

		let res = RequestProposalRes {
			request_id: RequestId(0),
			proposal: Some(proposal),
		};
		for address in addresses {
			if address != self.stream.address {
				// send proposal
				if let Some(peer_id) = self.stream.known_validators.get(&address) {
					self.stream.response(peer_id.clone(), res.clone())?;
				}

				// send prepare message
				self.stream
					.consensus_message(address, prepare_message.clone())?;
			}
		}
		Ok(())
	}

	fn on_prepare(&mut self, mut message: ConsensusMessage, address: Address) -> CommonResult<()> {
		match self.verify_message(&mut message, &address) {
			Err(_e) => return Ok(()),
			_ => {}
		}

		self.prepare_collector.add(address, message);
		let mut prepare_qc = self
			.prepare_collector
			.get_qc(&self.stream.authorities, self.stream.address.clone());

		if let Some(prepare_qc) = prepare_qc.take() {
			if self.pre_commit_message.is_none() {
				trace!("Prepare collected qc: {:?}", prepare_qc);
				self.stream.storage.update_prepare_qc(prepare_qc.clone())?;

				let pre_commit_message = ConsensusMessage {
					request_id: RequestId(0),
					message_type: MessageType::PreCommit,
					view: self.stream.storage.get_view(),
					node: None,
					justify: Some(prepare_qc),
					sig: None,
				};
				self.pre_commit_message = Some(pre_commit_message.clone());
				self.on_pre_commit(pre_commit_message.clone(), self.stream.address.clone())?;

				self.broadcast_message(pre_commit_message)?;
			}
		}
		Ok(())
	}

	fn on_pre_commit(
		&mut self,
		mut message: ConsensusMessage,
		address: Address,
	) -> CommonResult<()> {
		match self.verify_message(&mut message, &address) {
			Err(_e) => return Ok(()),
			_ => {}
		}

		self.pre_commit_collector.add(address, message);
		let mut pre_commit_qc = self
			.pre_commit_collector
			.get_qc(&self.stream.authorities, self.stream.address.clone());

		if let Some(pre_commit_qc) = pre_commit_qc.take() {
			if self.commit_message.is_none() {
				trace!("PreCommit collected qc: {:?}", pre_commit_qc);
				self.stream
					.storage
					.update_locked_qc(pre_commit_qc.clone())?;

				let commit_message = ConsensusMessage {
					request_id: RequestId(0),
					message_type: MessageType::Commit,
					view: self.stream.storage.get_view(),
					node: None,
					justify: Some(pre_commit_qc),
					sig: None,
				};

				self.commit_message = Some(commit_message.clone());
				self.on_commit(commit_message.clone(), self.stream.address.clone())?;

				self.broadcast_message(commit_message)?;
			}
		}
		Ok(())
	}

	fn on_commit(&mut self, mut message: ConsensusMessage, address: Address) -> CommonResult<()> {
		match self.verify_message(&mut message, &address) {
			Err(_e) => return Ok(()),
			_ => {}
		}

		self.commit_collector.add(address, message);
		let mut commit_qc = self
			.commit_collector
			.get_qc(&self.stream.authorities, self.stream.address.clone());

		if let Some(commit_qc) = commit_qc.take() {
			if self.decide_message.is_none() {
				trace!("Commit collected qc: {:?}", commit_qc);

				let decide_message = ConsensusMessage {
					request_id: RequestId(0),
					message_type: MessageType::Decide,
					view: self.stream.storage.get_view(),
					node: None,
					justify: Some(commit_qc.clone()),
					sig: None,
				};

				self.decide_message = Some(decide_message.clone());

				self.on_decide(commit_qc)?;

				self.broadcast_message(decide_message)?;
			}
		}
		Ok(())
	}

	fn on_decide(&mut self, commit_qc: QC) -> CommonResult<()> {
		let block_hash = &commit_qc.node.block_hash;
		let proposal = self.stream.storage.get_proposal(block_hash)?;

		let proposal = match proposal {
			Some(proposal) => proposal,
			None => return Ok(()),
		};

		let commit_block_params = match self.stream.commit_block_params.take() {
			Some(v) => {
				if v.block_hash == proposal.block_hash {
					v
				} else {
					return Ok(());
				}
			}
			None => return Ok(()),
		};

		self.stream.on_decide(commit_block_params, commit_qc)?;
		Ok(())
	}

	fn process_pending_new_view(&mut self) -> CommonResult<()> {
		let messages = self
			.stream
			.pending_new_view_messages
			.drain(..)
			.collect::<Vec<_>>();
		for (message, address) in messages {
			self.on_new_view(message, address)?;
		}
		Ok(())
	}

	fn send_new_view_to_self(&mut self) -> CommonResult<()> {
		let message = ConsensusMessage {
			request_id: RequestId(0),
			message_type: MessageType::NewView,
			view: self.stream.storage.get_view(),
			node: None,
			justify: Some(self.stream.storage.get_prepare_qc()),
			sig: None,
		};
		self.on_new_view(message, self.stream.address.clone())?;
		Ok(())
	}

	fn sign_message(&self, message: &mut ConsensusMessage) -> CommonResult<()> {
		let leader_address = self.stream.current_leader.clone().expect("qed");

		let sig_message = codec::encode(&(
			&message.message_type,
			&message.view,
			&message.node,
			&leader_address,
		))?;
		let sig = sign(&sig_message, &self.stream.secret_key, &self.stream.support)?;

		message.sig = Some((self.stream.public_key.clone(), sig));
		Ok(())
	}

	fn verify_message(
		&self,
		message: &mut ConsensusMessage,
		address: &Address,
	) -> CommonResult<()> {
		if &self.stream.address == address {
			message.node = self.prepare_message.as_ref().expect("qed").node.clone();
			message.justify = None;
			self.sign_message(message)?;
			return Ok(());
		}

		// verify sig
		let (public_key, signature) = match message.sig {
			Some(ref sig) => sig,
			None => return Err(ErrorKind::MessageError("Missing sig".to_string()).into()),
		};
		let expected_address = get_address(&public_key, &self.stream.support)?;
		if &expected_address != address {
			return Err(ErrorKind::MessageError("Unexpected address".to_string()).into());
		}
		let sig_message = codec::encode(&(
			&message.message_type,
			&message.view,
			&message.node,
			&self.stream.address,
		))?;
		verify(&sig_message, &public_key, &signature, &self.stream.support)?;

		// verify node
		if message.node != self.prepare_message.as_ref().expect("qed").node {
			return Err(ErrorKind::MessageError("Unexpected node".to_string()).into());
		}

		Ok(())
	}

	fn broadcast_message(&mut self, message: ConsensusMessage) -> CommonResult<()> {
		let addresses = self
			.stream
			.authorities
			.members
			.iter()
			.map(|x| x.0.clone())
			.collect::<Vec<_>>();

		for address in addresses {
			if address != self.stream.address {
				self.stream.consensus_message(address, message.clone())?;
			}
		}

		Ok(())
	}
}

pub struct ReplicaState<'a, S>
where
	S: ConsensusSupport,
{
	stream: &'a mut HotStuffStream<S>,
}

impl<'a, S> ReplicaState<'a, S>
where
	S: ConsensusSupport,
{
	pub fn new(stream: &'a mut HotStuffStream<S>) -> Self {
		Self { stream }
	}
	pub async fn start(mut self) -> CommonResult<()> {
		let current_view = self.stream.storage.get_view();

		self.send_new_view()?;

		loop {
			if self.stream.shutdown {
				return Ok(());
			}
			if self.stream.storage.get_view() != current_view {
				return Ok(());
			}

			let next_timeout = sleep_until(self.stream.next_timeout_instant());

			tokio::select! {
				_ = next_timeout => {
					self.stream.on_timeout()?;
				},
				Some(internal_message) = self.stream.internal_rx.next() => {
					match internal_message {
						InternalMessage::ValidatorRegistered { address, peer_id } => {
							self.on_validator_registered(address, peer_id)
								.unwrap_or_else(|e| warn!("HotStuff stream handle validator registered message error: {}", e));
						},
						InternalMessage::ConsensusMessage {message, peer_id } => {
							self.on_consensus_message(message, peer_id)
								.unwrap_or_else(|e| warn!("HotStuff stream handle consensus message error: {}", e));
						}
						_ => {
							self.stream.on_internal_message(internal_message)
								.unwrap_or_else(|e| warn!("HotStuff stream handle internal message error: {}", e));
						}
					}
				},
				in_message = self.stream.in_rx.next() => {
					match in_message {
						Some(in_message) => {
							self.stream.on_in_message(in_message)
								.unwrap_or_else(|e| warn!("HotStuff stream handle in message error: {}", e));
						},
						// in tx has been dropped
						None => {
							self.stream.shutdown = true;
						},
					}
				},
			}
		}
	}

	fn on_validator_registered(&mut self, address: Address, peer_id: PeerId) -> CommonResult<()> {
		if self.stream.current_leader.as_ref() == Some(&address) {
			self.send_new_view()?;
		}
		if let Some(messages) = self.stream.pending_consensus_messages.remove(&peer_id) {
			for message in messages {
				self.on_validator_consensus_message(message, address.clone())?;
			}
		}
		Ok(())
	}

	fn on_consensus_message(
		&mut self,
		message: ConsensusMessage,
		peer_id: PeerId,
	) -> CommonResult<()> {
		if let Some(address) = self.stream.get_peer_address(&peer_id) {
			if self
				.stream
				.authorities
				.members
				.iter()
				.any(|(addr, _)| addr == &address)
			{
				self.on_validator_consensus_message(message, address)?;
			}
		} else {
			let messages = self
				.stream
				.pending_consensus_messages
				.entry(peer_id)
				.or_insert_with(Vec::new);
			messages.push(message);
		}
		Ok(())
	}

	fn on_validator_consensus_message(
		&mut self,
		message: ConsensusMessage,
		address: Address,
	) -> CommonResult<()> {
		if let MessageType::NewView = &message.message_type {
			self.on_new_view(message.clone(), address.clone())?
		}

		if message.view != self.stream.storage.get_view() {
			return Ok(());
		}
		match &message.message_type {
			MessageType::Prepare => self.on_prepare(message, address)?,
			MessageType::PreCommit => self.on_pre_commit(message, address)?,
			MessageType::Commit => self.on_commit(message, address)?,
			MessageType::Decide => self.on_decide(message, address)?,
			_ => {}
		}
		Ok(())
	}

	fn on_new_view(&mut self, message: ConsensusMessage, address: Address) -> CommonResult<()> {
		self.stream
			.pending_new_view_messages
			.push((message, address));
		Ok(())
	}

	fn on_prepare(&mut self, message: ConsensusMessage, _address: Address) -> CommonResult<()> {
		let justify = match message.justify {
			Some(justify) => justify,
			None => return Ok(()),
		};

		let authorities = if justify.view == self.stream.storage.get_base_commit_qc().view {
			&self.stream.last_authorities
		} else {
			&self.stream.authorities
		};

		match verify_qc(&justify, None, &authorities, &self.stream.support) {
			Err(_e) => return Ok(()),
			_ => {}
		}

		let node = match &message.node {
			Some(node) => node,
			None => return Ok(()),
		};

		let block_hash = &node.block_hash;
		let proposal = self.stream.storage.get_proposal(block_hash)?;

		let proposal = match proposal {
			Some(proposal) => proposal,
			None => return Ok(()),
		};

		// check safe node
		if node.parent_hash != proposal.parent_hash {
			return Ok(());
		}

		let base_commit_qc = self.stream.storage.get_base_commit_qc();
		let locked_qc = self.stream.storage.get_locked_qc();

		let extend_from_justify_node = if justify.node.block_hash == base_commit_qc.node.block_hash
		{
			node.parent_hash == base_commit_qc.node.block_hash
		} else {
			node.block_hash == justify.node.block_hash
		};

		let extend_from_locked_qc_node =
			if locked_qc.node.block_hash == base_commit_qc.node.block_hash {
				node.parent_hash == base_commit_qc.node.block_hash
			} else {
				node.block_hash == locked_qc.node.block_hash
			};
		let higher_view = justify.view > locked_qc.view;

		let safe_node = extend_from_justify_node && (extend_from_locked_qc_node || higher_view);
		if !safe_node {
			warn!("Not safe node: {:?}", node);
			return Ok(());
		}

		let mut vote_message = ConsensusMessage {
			request_id: RequestId(0),
			message_type: message.message_type,
			view: message.view,
			node: message.node,
			justify: None,
			sig: None,
		};

		self.sign_message(&mut vote_message)?;

		let leader_address = self.stream.current_leader.clone().expect("qed");
		self.stream
			.consensus_message(leader_address, vote_message)?;
		Ok(())
	}

	fn on_pre_commit(&mut self, message: ConsensusMessage, _address: Address) -> CommonResult<()> {
		let justify = match message.justify {
			Some(justify) => justify,
			None => return Ok(()),
		};

		match verify_qc(
			&justify,
			Some(MessageType::Prepare),
			&self.stream.authorities,
			&self.stream.support,
		) {
			Err(_e) => return Ok(()),
			_ => {}
		}

		self.stream.storage.update_prepare_qc(justify.clone())?;

		let mut vote_message = ConsensusMessage {
			request_id: RequestId(0),
			message_type: message.message_type,
			view: message.view,
			node: Some(justify.node),
			justify: None,
			sig: None,
		};

		self.sign_message(&mut vote_message)?;

		let leader_address = self.stream.current_leader.clone().expect("qed");
		self.stream
			.consensus_message(leader_address, vote_message)?;

		Ok(())
	}

	fn on_commit(&mut self, message: ConsensusMessage, _address: Address) -> CommonResult<()> {
		let justify = match message.justify {
			Some(justify) => justify,
			None => return Ok(()),
		};

		match verify_qc(
			&justify,
			Some(MessageType::PreCommit),
			&self.stream.authorities,
			&self.stream.support,
		) {
			Err(_e) => return Ok(()),
			_ => {}
		}

		self.stream.storage.update_locked_qc(justify.clone())?;

		let mut vote_message = ConsensusMessage {
			request_id: RequestId(0),
			message_type: message.message_type,
			view: message.view,
			node: Some(justify.node),
			justify: None,
			sig: None,
		};

		self.sign_message(&mut vote_message)?;

		let leader_address = self.stream.current_leader.clone().expect("qed");
		self.stream
			.consensus_message(leader_address, vote_message)?;

		Ok(())
	}

	fn on_decide(&mut self, message: ConsensusMessage, _address: Address) -> CommonResult<()> {
		let justify = match message.justify {
			Some(justify) => justify,
			None => return Ok(()),
		};

		match verify_qc(
			&justify,
			Some(MessageType::Commit),
			&self.stream.authorities,
			&self.stream.support,
		) {
			Err(_e) => return Ok(()),
			_ => {}
		}

		let block_hash = &justify.node.block_hash;
		let proposal = self.stream.storage.get_proposal(block_hash)?;

		let proposal = match proposal {
			Some(proposal) => proposal,
			None => return Ok(()),
		};

		let commit_block_params = match self.stream.commit_block_params.take() {
			Some(v) => {
				if v.block_hash == proposal.block_hash {
					v
				} else {
					return Ok(());
				}
			}
			None => return Ok(()),
		};

		self.stream.on_decide(commit_block_params, justify)?;
		Ok(())
	}

	fn send_new_view(&mut self) -> CommonResult<()> {
		let leader_address = self.stream.current_leader.clone().expect("qed");
		let message = ConsensusMessage {
			request_id: RequestId(0),
			message_type: MessageType::NewView,
			view: self.stream.storage.get_view(),
			node: None,
			justify: Some(self.stream.storage.get_prepare_qc()),
			sig: None,
		};
		self.stream.consensus_message(leader_address, message)?;
		Ok(())
	}

	fn sign_message(&self, message: &mut ConsensusMessage) -> CommonResult<()> {
		let leader_address = self.stream.current_leader.clone().expect("qed");

		let sig_message = codec::encode(&(
			&message.message_type,
			&message.view,
			&message.node,
			&leader_address,
		))?;
		let sig = sign(&sig_message, &self.stream.secret_key, &self.stream.support)?;

		message.sig = Some((self.stream.public_key.clone(), sig));
		Ok(())
	}
}

pub struct ObserverState<'a, S>
where
	S: ConsensusSupport,
{
	stream: &'a mut HotStuffStream<S>,
}

impl<'a, S> ObserverState<'a, S>
where
	S: ConsensusSupport,
{
	pub fn new(stream: &'a mut HotStuffStream<S>) -> Self {
		Self { stream }
	}
	pub async fn start(mut self) -> CommonResult<()> {
		let current_view = self.stream.storage.get_view();
		loop {
			if self.stream.shutdown {
				return Ok(());
			}
			if self.stream.storage.get_view() != current_view {
				return Ok(());
			}
			tokio::select! {
				Some(internal_message) = self.stream.internal_rx.next() => {
					match internal_message {
						InternalMessage::ValidatorRegistered { address, peer_id } => {
							self.on_validator_registered(address, peer_id)
								.unwrap_or_else(|e| warn!("HotStuff stream handle validator registered message error: {}", e));
						},
						InternalMessage::ConsensusMessage {message, peer_id } => {
							self.on_consensus_message(message, peer_id)
								.unwrap_or_else(|e| warn!("HotStuff stream handle consensus message error: {}", e));
						}
						_ => {
							self.stream.on_internal_message(internal_message)
								.unwrap_or_else(|e| warn!("HotStuff stream handle internal message error: {}", e));
						}
					}
				},
				in_message = self.stream.in_rx.next() => {
					match in_message {
						Some(in_message) => {
							self.stream.on_in_message(in_message)
								.unwrap_or_else(|e| warn!("HotStuff stream handle in message error: {}", e));
						},
						// in tx has been dropped
						None => {
							self.stream.shutdown = true;
						},
					}
				},
			}
		}
	}

	fn on_validator_registered(&mut self, address: Address, peer_id: PeerId) -> CommonResult<()> {
		if let Some(messages) = self.stream.pending_consensus_messages.remove(&peer_id) {
			for message in messages {
				self.on_validator_consensus_message(message, address.clone())?;
			}
		}
		Ok(())
	}

	fn on_consensus_message(
		&mut self,
		message: ConsensusMessage,
		peer_id: PeerId,
	) -> CommonResult<()> {
		if let Some(address) = self.stream.get_peer_address(&peer_id) {
			if self
				.stream
				.authorities
				.members
				.iter()
				.any(|(addr, _)| addr == &address)
			{
				self.on_validator_consensus_message(message, address)?;
			}
		} else {
			let messages = self
				.stream
				.pending_consensus_messages
				.entry(peer_id)
				.or_insert_with(Vec::new);
			messages.push(message);
		}
		Ok(())
	}

	fn on_validator_consensus_message(
		&mut self,
		message: ConsensusMessage,
		address: Address,
	) -> CommonResult<()> {
		if let MessageType::NewView = &message.message_type {
			self.on_new_view(message, address)?
		}
		Ok(())
	}

	fn on_new_view(&mut self, message: ConsensusMessage, address: Address) -> CommonResult<()> {
		self.stream
			.pending_new_view_messages
			.push((message, address));
		Ok(())
	}
}

struct ConsensusMessageCollector {
	messages: HashMap<Address, ConsensusMessage>,
}

impl ConsensusMessageCollector {
	fn new() -> Self {
		Self {
			messages: Default::default(),
		}
	}
	fn add(&mut self, address: Address, message: ConsensusMessage) {
		self.messages.insert(address, message);
	}
	fn get_high_qc(&self, authorities: &Authorities) -> Option<QC> {
		let n = authorities.members.iter().fold(0u32, |sum, x| sum + x.1);
		let f = (n - 1) / 3;
		let member_weight = authorities
			.members
			.iter()
			.map(|x| (&x.0, x.1))
			.collect::<HashMap<_, _>>();

		let mut total_weight = 0;
		let mut high_qc = None;
		for (address, message) in &self.messages {
			let justify = message.justify.as_ref().expect("qed");
			if let Some(weight) = member_weight.get(address) {
				total_weight += weight;
				high_qc = match high_qc {
					None => Some(justify.clone()),
					Some(high_qc) => {
						if justify.view > high_qc.view {
							Some(justify.clone())
						} else {
							Some(high_qc)
						}
					}
				}
			}
			if total_weight >= n - f {
				return high_qc;
			}
		}
		None
	}

	fn get_high_qc_addresses(&self, qc: &QC) -> Vec<Address> {
		self.messages
			.iter()
			.filter_map(|(address, message)| {
				if message.justify.as_ref() == Some(qc) {
					Some(address.clone())
				} else {
					None
				}
			})
			.collect()
	}

	fn get_qc(&self, authorities: &Authorities, leader_address: Address) -> Option<QC> {
		let n = authorities.members.iter().fold(0u32, |sum, x| sum + x.1);
		let f = (n - 1) / 3;
		let member_weight = authorities
			.members
			.iter()
			.map(|x| (&x.0, x.1))
			.collect::<HashMap<_, _>>();

		let mut total_weight = 0;
		let mut qc_sig = vec![];
		for (address, message) in &self.messages {
			if let Some(weight) = member_weight.get(address) {
				total_weight += weight;
				qc_sig.push(message.sig.clone().expect("qed"));
			}
			if total_weight >= n - f {
				let qc = QC {
					message_type: message.message_type.clone(),
					view: message.view,
					node: message.node.clone().expect("qed"),
					leader_address,
					sig: qc_sig,
				};
				return Some(qc);
			}
		}
		None
	}
}
