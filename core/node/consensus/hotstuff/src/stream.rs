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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use log::{info, trace};

use crypto::address::Address as AddressT;
use crypto::dsa::Dsa;
use crypto::dsa::KeyPair;
use crypto::dsa::Verifier;
use node_consensus_base::support::ConsensusSupport;
use node_consensus_base::{ConsensusInMessage, ConsensusOutMessage, PeerId};
use node_executor::module::hotstuff::{Authorities, Meta};
use primitives::codec::Encode;
use primitives::errors::{Catchable, CommonResult};
use primitives::{codec, BlockNumber, Hash};
use primitives::{Address, PublicKey, SecretKey, Signature};

use crate::config::DEFAULT_INIT_EXTRA_TIMEOUT;
use crate::errors::ErrorKind;
use crate::proof::Proof;
use crate::protocol::{
	ConsensusMessage, HotStuffMessage, MessageType, Proposal, RegisterValidatorReq,
	RegisterValidatorRes, RequestId, RequestIdAware, RequestProposalReq, RequestProposalRes,
	TimeoutMessage, TimeoutMessageType, QC, TC,
};
use crate::storage::Storage;
use crate::stream::state::{LeaderState, ObserverState, ReplicaState, State};
use crate::verifier::VerifyError;
use crate::{get_hotstuff_authorities, HotStuffConfig};
use node_chain::ChainCommitBlockParams;
use node_consensus_base::errors::map_channel_err;
use node_consensus_primitives::CONSENSUS_HOTSTUFF;
use serde::Serialize;
use serde_json::Value;
use std::convert::TryInto;
use tokio::time::{Duration, Instant};

mod state;

pub struct HotStuffStream<S>
where
	S: ConsensusSupport,
{
	/// External support
	support: Arc<S>,

	/// HotStuff meta data
	hotstuff_meta: Arc<Meta>,

	/// HotStuff config
	hotstuff_config: Arc<HotStuffConfig>,

	/// Secret key of current validator
	secret_key: SecretKey,

	/// Public key of current validator
	public_key: PublicKey,

	/// Address of current validator
	address: Address,

	/// Out message sender
	out_tx: UnboundedSender<ConsensusOutMessage>,

	/// In message receiver
	in_rx: UnboundedReceiver<ConsensusInMessage>,

	/// All connected peers
	peers: HashMap<PeerId, PeerInfo>,

	/// Known validators registered to us
	known_validators: HashMap<Address, PeerId>,

	/// Next request id
	next_request_id: RequestId,

	/// Persistant storage
	storage: Arc<Storage<S>>,

	/// Next election instant
	next_timeout_instant: Option<Instant>,

	/// HotStuff current leader
	current_leader: Option<Address>,

	/// HotStuff state
	state: State,

	/// Authorities
	authorities: Authorities,

	/// Last authorities
	last_authorities: Authorities,

	/// Internal message sender
	internal_tx: UnboundedSender<InternalMessage>,

	/// Internal message receiver
	internal_rx: UnboundedReceiver<InternalMessage>,

	/// Commit block params
	/// When building or verifying a proposal, the commit_block_params
	/// is returned. We just keep it for later use
	commit_block_params: Option<ChainCommitBlockParams>,

	/// pending consensus messages from peer
	/// having not registered as known validator
	pending_consensus_messages: HashMap<PeerId, Vec<ConsensusMessage>>,

	/// pending new view messages from replica
	/// when current view has not shift
	pending_new_view_messages: Vec<(ConsensusMessage, Address)>,

	/// timeout message collector
	timeout_message_collector: TimeoutMessageCollector,

	/// timeout tc
	timeout_tc: Option<TC>,

	/// Shutdown
	shutdown: bool,
}

impl<S> HotStuffStream<S>
where
	S: ConsensusSupport,
{
	pub fn spawn(
		support: Arc<S>,
		hotstuff_meta: Meta,
		hotstuff_config: HotStuffConfig,
		out_tx: UnboundedSender<ConsensusOutMessage>,
		in_rx: UnboundedReceiver<ConsensusInMessage>,
	) -> CommonResult<()> {
		let secret_key = match &hotstuff_config.secret_key {
			Some(v) => v.clone(),
			None => return Ok(()),
		};

		let public_key = get_public_key(&secret_key, &support)?;
		let address = get_address(&public_key, &support)?;
		let hotstuff_meta = Arc::new(hotstuff_meta);
		let hotstuff_config = Arc::new(hotstuff_config);
		let storage = Arc::new(Storage::new(support.clone())?);
		let confirmed_number = support.get_current_state().confirmed_number;
		let authorities = get_hotstuff_authorities(&support, &confirmed_number)?;
		let last_confirmed_number = if confirmed_number > 0 {
			confirmed_number - 1
		} else {
			0
		};
		let last_authorities = get_hotstuff_authorities(&support, &last_confirmed_number)?;

		let (internal_tx, internal_rx) = unbounded();

		let this = Self {
			support,
			hotstuff_meta,
			hotstuff_config,
			secret_key,
			public_key,
			address,
			out_tx,
			in_rx,
			peers: HashMap::new(),
			known_validators: HashMap::new(),
			next_request_id: RequestId(0),
			storage,
			next_timeout_instant: None,
			current_leader: None,
			state: State::Observer,
			authorities,
			last_authorities,
			internal_tx,
			internal_rx,
			commit_block_params: None,
			pending_consensus_messages: HashMap::new(),
			pending_new_view_messages: Vec::new(),
			timeout_message_collector: TimeoutMessageCollector::new(),
			timeout_tc: None,
			shutdown: false,
		};
		tokio::spawn(this.start());
		Ok(())
	}

	async fn start(mut self) -> CommonResult<()> {
		info!("Start hotstuff work");

		let init_extra_timeout = self
			.hotstuff_config
			.init_extra_timeout
			.unwrap_or(DEFAULT_INIT_EXTRA_TIMEOUT);
		self.update_next_timeout_instant(init_extra_timeout);

		loop {
			if self.shutdown {
				return Ok(());
			}
			self.run_view().await?;
		}
	}

	async fn run_view(&mut self) -> CommonResult<()> {
		let current_view = self.storage.get_view();
		let current_leader = self.compute_leader();
		if self.address == current_leader {
			self.update_state(State::Leader);
		} else {
			self.update_state(State::Replica);
		}
		info!(
			"Work as {:?}, current_view: {}, current_leader: {}",
			self.state, current_view, current_leader
		);

		self.update_current_leader(Some(current_leader));

		match self.state {
			State::Leader => LeaderState::new(self).start().await?,
			State::Replica => ReplicaState::new(self).start().await?,
			State::Observer => ObserverState::new(self).start().await?,
		}

		Ok(())
	}

	fn compute_leader(&self) -> Address {
		let last_leader_index = if self.last_authorities == self.authorities {
			let base_commit_qc = self.storage.get_base_commit_qc();
			let leader_address = base_commit_qc.leader_address;
			let index = self
				.authorities
				.members
				.iter()
				.position(|(address, _)| address == &leader_address)
				.expect("base commit qc is verified");
			index
		} else {
			self.authorities.members.len() - 1
		};

		let view = self.storage.get_view();
		let base_commit_qc_view = self.storage.get_base_commit_qc().view;
		let view_diff = view - base_commit_qc_view;

		let leader_index = last_leader_index + view_diff as usize;

		self.authorities.members[leader_index % self.authorities.members.len()]
			.0
			.clone()
	}

	fn next_timeout_instant(&mut self) -> Instant {
		match self.next_timeout_instant {
			Some(inst) => inst,
			None => {
				let inst = Instant::now() + Duration::from_millis(self.hotstuff_meta.view_timeout);
				self.next_timeout_instant = Some(inst);
				inst
			}
		}
	}

	fn update_next_timeout_instant(&mut self, extra: u64) {
		let now = Instant::now();
		self.next_timeout_instant = Some(
			now + Duration::from_millis(self.hotstuff_meta.view_timeout)
				+ Duration::from_millis(extra),
		);
	}

	fn update_current_leader(&mut self, leader: Option<Address>) {
		self.current_leader = leader;
	}

	fn update_state(&mut self, state: State) {
		if state == State::Replica
			&& !self
				.authorities
				.members
				.iter()
				.any(|(address, _)| address == &self.address)
		{
			self.state = State::Observer;
			return;
		}
		self.state = state;
	}

	fn consensus_state(&self) -> CommonResult<ConsensusState> {
		Ok(ConsensusState {
			consensus_name: CONSENSUS_HOTSTUFF.to_string(),
			address: self.address.clone(),
			meta: (*self.hotstuff_meta).clone(),
			authorities: self.authorities.clone(),
			current_leader: self.current_leader.clone(),
		})
	}
}

/// methods for in messages
impl<S> HotStuffStream<S>
where
	S: ConsensusSupport,
{
	fn on_in_message(&mut self, in_message: ConsensusInMessage) -> CommonResult<()> {
		match in_message {
			ConsensusInMessage::NetworkProtocolOpen {
				peer_id,
				local_nonce,
				remote_nonce,
			} => {
				self.on_network_protocol_open(peer_id, local_nonce, remote_nonce)?;
			}
			ConsensusInMessage::NetworkProtocolClose { peer_id } => {
				self.on_network_protocol_close(peer_id);
			}
			ConsensusInMessage::NetworkMessage { peer_id, message } => {
				self.on_network_message(peer_id, message)?;
			}
			ConsensusInMessage::BlockCommitted { number, block_hash } => {
				self.on_block_committed(number, block_hash)?;
			}
			ConsensusInMessage::GetConsensusState { tx } => {
				let value = serde_json::to_value(self.consensus_state()?).unwrap_or(Value::Null);
				let _ = tx.send(value);
			}
			_ => {}
		}
		Ok(())
	}

	fn on_network_protocol_open(
		&mut self,
		peer_id: PeerId,
		local_nonce: u64,
		remote_nonce: u64,
	) -> CommonResult<()> {
		self.peers.insert(
			peer_id.clone(),
			PeerInfo {
				local_nonce,
				remote_nonce,
				address: None,
			},
		);
		self.register_validator(peer_id)?;
		Ok(())
	}

	fn on_network_protocol_close(&mut self, peer_id: PeerId) {
		if let Some(peer_info) = self.peers.get(&peer_id) {
			if let Some(address) = &peer_info.address {
				self.known_validators.remove(address);
			}
		}
		self.peers.remove(&peer_id);
	}

	fn on_network_message(&mut self, peer_id: PeerId, message: Vec<u8>) -> CommonResult<()> {
		let message: HotStuffMessage = codec::decode(&mut &message[..])?;

		match message {
			HotStuffMessage::RegisterValidatorReq(req) => {
				let res = self.on_req_register_validator(peer_id.clone(), req)?;
				self.response(peer_id, res)?;
			}
			HotStuffMessage::RegisterValidatorRes(res) => {
				self.on_res_register_validator(peer_id, res)?;
			}
			HotStuffMessage::RequestProposalReq(req) => {
				if let Some(address) = self.get_peer_address(&peer_id) {
					let res = self.on_req_request_proposal(address, req)?;
					self.response(peer_id, res)?;
				}
			}
			HotStuffMessage::RequestProposalRes(res) => {
				if let Some(address) = self.get_peer_address(&peer_id) {
					self.on_res_request_proposal(address, res)?;
				}
			}
			HotStuffMessage::ConsensusMessage(message) => {
				self.internal_tx
					.unbounded_send(InternalMessage::ConsensusMessage { message, peer_id })
					.map_err(map_channel_err)?;
			}
			HotStuffMessage::TimeoutMessage(message) => {
				if let Some(address) = self.get_peer_address(&peer_id) {
					self.on_timeout_message(message, address)?;
				}
			}
		};
		Ok(())
	}
}

#[allow(clippy::single_match)]
impl<S> HotStuffStream<S>
where
	S: ConsensusSupport,
{
	fn on_internal_message(&mut self, _internal_message: InternalMessage) -> CommonResult<()> {
		Ok(())
	}

	fn on_decide(
		&mut self,
		mut commit_block_params: ChainCommitBlockParams,
		commit_qc: QC,
	) -> CommonResult<()> {
		let proof = Proof::new(commit_qc)?;
		commit_block_params.proof = proof.try_into()?;

		let number = commit_block_params.header.number;
		let block_hash = commit_block_params.block_hash.clone();

		let tx_hash_set = commit_block_params
			.body
			.meta_txs
			.iter()
			.chain(commit_block_params.body.payload_txs.iter())
			.cloned()
			.collect::<HashSet<_>>();

		self.support.commit_block(commit_block_params)?;

		self.support.txpool_remove_transactions(&tx_hash_set)?;

		info!(
			"Block committed: number: {}, block_hash: {}",
			number, block_hash
		);

		Ok(())
	}

	fn on_block_committed(&mut self, number: BlockNumber, block_hash: Hash) -> CommonResult<()> {
		self.storage.refresh()?;
		info!(
			"Storage refreshed: number: {}, block_hash: {}",
			number, block_hash
		);

		let confirmed_number = self.support.get_current_state().confirmed_number;
		let authorities = get_hotstuff_authorities(&self.support, &confirmed_number)?;
		self.last_authorities = std::mem::replace(&mut self.authorities, authorities);
		Ok(())
	}
}

impl<S> HotStuffStream<S>
where
	S: ConsensusSupport,
{
	fn on_timeout(&mut self) -> CommonResult<()> {
		let mut message = TimeoutMessage {
			request_id: RequestId(0),
			message_type: TimeoutMessageType::NewTimeout,
			view: self.storage.get_view(),
			justify: self.timeout_tc.clone(),
			sig: None,
		};

		let sig_message = codec::encode(&(&message.message_type, &message.view))?;
		let sig = sign(&sig_message, &self.secret_key, &self.support)?;

		message.sig = Some((self.public_key.clone(), sig));

		let addresses = self
			.authorities
			.members
			.iter()
			.map(|x| x.0.clone())
			.collect::<Vec<_>>();

		self.on_timeout_message(message.clone(), self.address.clone())?;

		for address in addresses {
			if address != self.address {
				self.timeout_message(address, message.clone())?;
			}
		}

		self.update_next_timeout_instant(0);

		Ok(())
	}

	fn on_timeout_message(
		&mut self,
		message: TimeoutMessage,
		address: Address,
	) -> CommonResult<()> {
		if message.view < self.storage.get_view() {
			return Ok(());
		}

		match &message.message_type {
			TimeoutMessageType::NewTimeout => self.on_timeout_new_timeout(message, address)?,
			TimeoutMessageType::Decide => self.on_timeout_decide(message, address)?,
		}
		Ok(())
	}

	fn on_timeout_new_timeout(
		&mut self,
		message: TimeoutMessage,
		address: Address,
	) -> CommonResult<()> {
		// verify sig
		let (public_key, signature) = match message.sig {
			Some(ref sig) => sig,
			None => return Ok(()),
		};

		let expected_address = get_address(&public_key, &self.support)?;
		if expected_address != address {
			return Ok(());
		}

		let sig_message = codec::encode(&(&message.message_type, &message.view))?;
		let result = verify(&sig_message, &public_key, &signature, &self.support);
		if result.is_err() {
			return Ok(());
		}

		// process tc
		if let Some(justify) = &message.justify {
			if justify.view >= self.storage.get_view() {
				match verify_tc(justify, &self.authorities, &self.support) {
					Err(_e) => return Ok(()),
					_ => {}
				}
				self.advance_view(justify.view)?;
			}
		}

		let view = message.view;
		self.timeout_message_collector.add(address, message);
		let mut tc = self
			.timeout_message_collector
			.get_tc(&self.authorities, view);

		if let Some(tc) = tc.take() {
			self.timeout_tc = Some(tc.clone());

			self.advance_view(tc.view)?;

			let decide_message = TimeoutMessage {
				request_id: RequestId(0),
				message_type: TimeoutMessageType::Decide,
				view: tc.view,
				justify: Some(tc),
				sig: None,
			};

			let addresses = self
				.authorities
				.members
				.iter()
				.map(|x| x.0.clone())
				.collect::<Vec<_>>();

			for address in addresses {
				if address != self.address {
					self.timeout_message(address, decide_message.clone())?;
				}
			}
		}

		Ok(())
	}

	fn on_timeout_decide(
		&mut self,
		message: TimeoutMessage,
		_address: Address,
	) -> CommonResult<()> {
		let justify = match message.justify {
			Some(justify) => justify,
			None => return Ok(()),
		};

		match verify_tc(&justify, &self.authorities, &self.support) {
			Err(_e) => return Ok(()),
			_ => {}
		}

		self.advance_view(justify.view)?;

		Ok(())
	}

	fn advance_view(&mut self, view: u64) -> CommonResult<()> {
		let next_view = view + 1;
		info!("Advance view when timeout: next_view: {}", next_view);
		self.storage.update_view(next_view)?;
		self.timeout_message_collector.cleanup(view);
		Ok(())
	}
}

/// methods for out messages
impl<S> HotStuffStream<S>
where
	S: ConsensusSupport,
{
	fn request<Req>(&mut self, peer_id: PeerId, mut request: Req) -> CommonResult<RequestId>
	where
		Req: RequestIdAware + Into<HotStuffMessage>,
	{
		let request_id = Self::next_request_id(&mut self.next_request_id);
		request.set_request_id(request_id.clone());

		let message = request.into();
		let out_message = ConsensusOutMessage::NetworkMessage {
			peer_id,
			message: message.encode(),
		};
		self.send_out_message(out_message)?;

		Ok(request_id)
	}

	fn get_peer_address(&self, peer_id: &PeerId) -> Option<Address> {
		match self.peers.get(peer_id) {
			Some(peer_info) => peer_info.address.clone(),
			None => None,
		}
	}

	fn response<Res>(&self, peer_id: PeerId, res: Res) -> CommonResult<()>
	where
		Res: Into<HotStuffMessage>,
	{
		let message = res.into();
		let out_message = ConsensusOutMessage::NetworkMessage {
			peer_id,
			message: message.encode(),
		};
		self.send_out_message(out_message)?;
		Ok(())
	}

	fn next_request_id(request_id: &mut RequestId) -> RequestId {
		let new = RequestId(request_id.0.checked_add(1).unwrap_or(0));
		std::mem::replace(request_id, new)
	}

	fn send_out_message(&self, out_message: ConsensusOutMessage) -> CommonResult<()> {
		self.out_tx
			.unbounded_send(out_message)
			.map_err(map_channel_err)?;
		Ok(())
	}
}

impl<S> HotStuffStream<S>
where
	S: ConsensusSupport,
{
	fn register_validator(&mut self, peer_id: PeerId) -> CommonResult<Option<RequestId>> {
		let peer_info = match self.peers.get(&peer_id) {
			Some(v) => v,
			None => return Ok(None),
		};
		trace!("Request validator: peer_id: {}", peer_id);

		let message = codec::encode(&peer_info.remote_nonce)?;

		let signature = sign(&message, &self.secret_key, &self.support)?;

		let req = RegisterValidatorReq {
			request_id: RequestId(0),
			public_key: self.public_key.clone(),
			signature,
		};
		let request_id = self.request(peer_id.clone(), req)?;

		Ok(Some(request_id))
	}

	fn request_proposal(
		&mut self,
		address: Address,
		req: RequestProposalReq,
	) -> CommonResult<Option<RequestId>> {
		let peer_id = match self.known_validators.get(&address) {
			Some(v) => v,
			None => return Ok(None),
		};
		trace!("Request proposal: address: {}, req: {:?}", address, req);

		let peer_id = peer_id.clone();
		let request_id = self.request(peer_id, req)?;

		Ok(Some(request_id))
	}

	fn consensus_message(
		&mut self,
		address: Address,
		req: ConsensusMessage,
	) -> CommonResult<Option<RequestId>> {
		let peer_id = match self.known_validators.get(&address) {
			Some(v) => v,
			None => return Ok(None),
		};
		trace!("Consensus message: address: {}, req: {:?}", address, req);

		let peer_id = peer_id.clone();
		let request_id = self.request(peer_id, req)?;

		Ok(Some(request_id))
	}

	fn timeout_message(
		&mut self,
		address: Address,
		req: TimeoutMessage,
	) -> CommonResult<Option<RequestId>> {
		let peer_id = match self.known_validators.get(&address) {
			Some(v) => v,
			None => return Ok(None),
		};
		trace!("Timeout message: address: {}, req: {:?}", address, req);

		let peer_id = peer_id.clone();
		let request_id = self.request(peer_id, req)?;

		Ok(Some(request_id))
	}

	fn on_req_register_validator(
		&mut self,
		peer_id: PeerId,
		req: RegisterValidatorReq,
	) -> CommonResult<RegisterValidatorRes> {
		trace!(
			"On req register validator: peer_id: {}, req: {:?}",
			peer_id,
			req
		);

		let mut success = false;
		if let Some(peer_info) = self.peers.get_mut(&peer_id) {
			let message = codec::encode(&peer_info.local_nonce)?;
			let result = verify(&message, &req.public_key, &req.signature, &self.support);
			success = result.is_ok();

			if success {
				let address = get_address(&req.public_key, &self.support)?;
				info!(
					"Register validator accepted: peer_id: {}, address: {}",
					peer_id, address
				);
				peer_info.address = Some(address.clone());
				self.known_validators
					.insert(address.clone(), peer_id.clone());
				self.internal_tx
					.unbounded_send(InternalMessage::ValidatorRegistered { address, peer_id })
					.map_err(map_channel_err)?;
			}
		}

		Ok(RegisterValidatorRes {
			request_id: req.request_id,
			success,
		})
	}

	fn on_res_register_validator(
		&mut self,
		peer_id: PeerId,
		res: RegisterValidatorRes,
	) -> CommonResult<()> {
		trace!(
			"On res register validator: peer_id: {}, res: {:?}",
			peer_id,
			res
		);
		Ok(())
	}

	fn on_req_request_proposal(
		&mut self,
		address: Address,
		req: RequestProposalReq,
	) -> CommonResult<RequestProposalRes> {
		trace!(
			"On req request proposal: address: {}, req: {:?}",
			address,
			req
		);

		let proposal = self.storage.get_proposal(&req.block_hash)?.ok_or_else(|| {
			node_consensus_base::errors::ErrorKind::Data(
				"Missing proposal on req request proposal".to_string(),
			)
		})?;

		// send proposal
		Ok(RequestProposalRes {
			request_id: req.request_id,
			proposal: Some(proposal),
		})
	}

	fn on_res_request_proposal(
		&mut self,
		address: Address,
		res: RequestProposalRes,
	) -> CommonResult<()> {
		trace!(
			"On res request proposal: address: {}, res: {:?}",
			address,
			res
		);
		if let Some(proposal) = res.proposal {
			self.on_proposal(proposal.clone())?;
			self.internal_tx
				.unbounded_send(InternalMessage::Proposal { address, proposal })
				.map_err(map_channel_err)?;
		}

		Ok(())
	}

	fn on_proposal(&mut self, proposal: Proposal) -> CommonResult<()> {
		let verifier = crate::verifier::Verifier::new(self.support.clone())?;
		let mut proposal = Some(proposal);
		let result = verifier.verify_proposal(&mut proposal);
		let result_desc = match &result {
			Ok(_v) => "Ok".to_string(),
			Err(e) => e.to_string(),
		};
		trace!("Proposal verify result: {}", result_desc);
		let action = self.on_proposal_verify_result(result)?;
		match action {
			VerifyAction::Ok(proposal, commit_block_params) => {
				self.commit_block_params = Some(commit_block_params);
				self.storage.put_proposal(proposal)?;
			}
			VerifyAction::Wait => {
				// proposal has not been taken
			}
			VerifyAction::Discard => {}
		}
		Ok(())
	}

	fn on_proposal_verify_result(
		&self,
		result: CommonResult<(Proposal, ChainCommitBlockParams)>,
	) -> CommonResult<VerifyAction> {
		let action = result
			.and_then(|(proposal, commit_block_params)| {
				self.on_proposal_verify_ok(proposal, commit_block_params)
			})
			.or_else_catch::<ErrorKind, _>(|e| match e {
				ErrorKind::VerifyError(e) => Some(self.on_proposal_verify_err(e)),
				_ => None,
			})?;
		Ok(action)
	}

	fn on_proposal_verify_ok(
		&self,
		proposal: Proposal,
		commit_block_params: ChainCommitBlockParams,
	) -> CommonResult<VerifyAction> {
		let action = VerifyAction::Ok(proposal, commit_block_params);
		Ok(action)
	}

	fn on_proposal_verify_err(&self, e: &VerifyError) -> CommonResult<VerifyAction> {
		let action = match e {
			VerifyError::ShouldWait => VerifyAction::Wait,
			VerifyError::Duplicated => VerifyAction::Discard,
			VerifyError::NotBest => VerifyAction::Discard,
			VerifyError::InvalidExecutionGap => VerifyAction::Discard,
			VerifyError::InvalidHeader(_) => VerifyAction::Discard,
			VerifyError::DuplicatedTx(_) => VerifyAction::Discard,
			VerifyError::InvalidTx(_) => VerifyAction::Discard,
		};
		Ok(action)
	}
}

fn get_public_key<S: ConsensusSupport>(
	secret_key: &SecretKey,
	support: &Arc<S>,
) -> CommonResult<PublicKey> {
	let dsa = support.get_basic()?.dsa.clone();
	let (_, public_key_len, _) = dsa.length().into();
	let mut public_key = vec![0u8; public_key_len];
	dsa.key_pair_from_secret_key(&secret_key.0)?
		.public_key(&mut public_key);

	let public_key = PublicKey(public_key);
	Ok(public_key)
}

fn get_address<S: ConsensusSupport>(
	public_key: &PublicKey,
	support: &Arc<S>,
) -> CommonResult<Address> {
	let addresser = support.get_basic()?.address.clone();
	let address_len = addresser.length().into();
	let mut address = vec![0u8; address_len];
	addresser.address(&mut address, &public_key.0);

	let address = Address(address);
	Ok(address)
}

fn sign<S: ConsensusSupport>(
	message: &[u8],
	secret_key: &SecretKey,
	support: &Arc<S>,
) -> CommonResult<Signature> {
	let dsa = support.get_basic()?.dsa.clone();
	let signature = {
		let keypair = dsa.key_pair_from_secret_key(&secret_key.0)?;
		let (_, _, signature_len) = dsa.length().into();
		let mut out = vec![0u8; signature_len];
		keypair.sign(message, &mut out);
		Signature(out)
	};
	Ok(signature)
}

fn verify<S: ConsensusSupport>(
	message: &[u8],
	public_key: &PublicKey,
	signature: &Signature,
	support: &Arc<S>,
) -> CommonResult<()> {
	let dsa = support.get_basic()?.dsa.clone();
	let verifier = dsa.verifier_from_public_key(&public_key.0)?;
	verifier.verify(&message, &signature.0)
}

pub fn verify_qc<S: ConsensusSupport>(
	qc: &QC,
	expected_message_type: Option<MessageType>,
	authorities: &Authorities,
	support: &Arc<S>,
) -> CommonResult<()> {
	// genesis qc
	if qc.view == 0 {
		return Ok(());
	}

	if let Some(expected_message_type) = expected_message_type {
		if qc.message_type != expected_message_type {
			return Err(ErrorKind::QCError("Unexpected message type".to_string()).into());
		}
	}

	let member_weight = authorities
		.members
		.iter()
		.map(|x| (&x.0, x.1))
		.collect::<HashMap<_, _>>();

	let mut total_weight = 0;
	for (public_key, signature) in &qc.sig {
		let address = get_address(public_key, &support)?;

		if let Some(weight) = member_weight.get(&address) {
			total_weight += weight;
		}

		let sig_message = codec::encode(&(
			&qc.message_type,
			&qc.view,
			Some(&qc.node),
			&qc.leader_address,
		))?;

		verify(&sig_message, &public_key, &signature, &support)
			.map_err(|e| ErrorKind::QCError(format!("{}", e)))?;
	}

	let n = authorities.members.iter().fold(0u32, |sum, x| sum + x.1);
	let f = (n - 1) / 3;

	if total_weight < n - f {
		return Err(ErrorKind::QCError("No enough weight".to_string()).into());
	}

	Ok(())
}

pub fn verify_tc<S: ConsensusSupport>(
	tc: &TC,
	authorities: &Authorities,
	support: &Arc<S>,
) -> CommonResult<()> {
	let member_weight = authorities
		.members
		.iter()
		.map(|x| (&x.0, x.1))
		.collect::<HashMap<_, _>>();

	let mut total_weight = 0;
	for (public_key, signature) in &tc.sig {
		let address = get_address(public_key, &support)?;

		if let Some(weight) = member_weight.get(&address) {
			total_weight += weight;
		}

		let sig_message = codec::encode(&(&TimeoutMessageType::NewTimeout, &tc.view))?;

		verify(&sig_message, &public_key, &signature, &support)
			.map_err(|e| ErrorKind::QCError(format!("{}", e)))?;
	}

	let n = authorities.members.iter().fold(0u32, |sum, x| sum + x.1);
	let f = (n - 1) / 3;

	if total_weight < n - f {
		return Err(ErrorKind::QCError("No enough weight".to_string()).into());
	}

	Ok(())
}

struct PeerInfo {
	local_nonce: u64,
	remote_nonce: u64,
	address: Option<Address>,
}

#[derive(Debug)]
enum VerifyAction {
	Ok(Proposal, ChainCommitBlockParams),
	Wait,
	Discard,
}

#[allow(clippy::large_enum_variant)]
enum InternalMessage {
	ValidatorRegistered {
		address: Address,
		peer_id: PeerId,
	},
	ConsensusMessage {
		message: ConsensusMessage,
		peer_id: PeerId,
	},
	Proposal {
		address: Address,
		proposal: Proposal,
	},
}

struct TimeoutMessageCollector {
	messages: HashMap<u64, HashMap<Address, TimeoutMessage>>,
}

impl TimeoutMessageCollector {
	fn new() -> Self {
		Self {
			messages: Default::default(),
		}
	}
	fn add(&mut self, address: Address, timeout_message: TimeoutMessage) {
		self.messages
			.entry(timeout_message.view)
			.or_insert_with(HashMap::new)
			.entry(address)
			.or_insert_with(|| timeout_message);
	}

	fn get_tc(&self, authorities: &Authorities, view: u64) -> Option<TC> {
		let n = authorities.members.iter().fold(0u32, |sum, x| sum + x.1);
		let f = (n - 1) / 3;
		let member_weight = authorities
			.members
			.iter()
			.map(|x| (&x.0, x.1))
			.collect::<HashMap<_, _>>();

		let mut total_weight = 0;
		let mut tc_sig = vec![];
		if let Some(messages) = self.messages.get(&view) {
			for (address, message) in messages {
				if let Some(weight) = member_weight.get(address) {
					total_weight += weight;
					tc_sig.push(message.sig.clone().expect("qed"));
				}
				if total_weight >= n - f {
					let tc = TC {
						view: message.view,
						sig: tc_sig,
					};
					return Some(tc);
				}
			}
		}
		None
	}

	pub fn cleanup(&mut self, view: u64) {
		self.messages.retain(|k, _| k > &view);
	}
}

#[derive(Serialize)]
struct ConsensusState {
	consensus_name: String,
	address: Address,
	meta: Meta,
	authorities: Authorities,
	current_leader: Option<Address>,
}
