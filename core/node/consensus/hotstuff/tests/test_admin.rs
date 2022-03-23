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

use crypto::address::AddressImpl;
use crypto::dsa::DsaImpl;
use log::info;
use node_consensus_base::PeerId;
use node_coordinator::{Keypair, LinkedHashMap, Multiaddr, Protocol};
use node_executor::module;
use node_executor::module::hotstuff::Authorities;
use node_executor_primitives::EmptyParams;
use utils_test::test_accounts;

mod base;

#[tokio::test]
async fn test_hotstuff_update_admin() {
	let _ = env_logger::try_init();

	let dsa = Arc::new(DsaImpl::Ed25519);
	let address = Arc::new(AddressImpl::Blake2b160);

	let test_accounts = test_accounts(dsa.clone(), address);
	let (account1, account2, account3, account4) = (
		&test_accounts[0],
		&test_accounts[1],
		&test_accounts[2],
		&test_accounts[3],
	);

	let authority_accounts = [account1, account2, account3, account4];

	let specs = vec![
		(
			authority_accounts,
			account1.clone(),
			Keypair::generate_ed25519(),
			1509,
		),
		(
			authority_accounts,
			account2.clone(),
			Keypair::generate_ed25519(),
			1510,
		),
		(
			authority_accounts,
			account3.clone(),
			Keypair::generate_ed25519(),
			1511,
		),
		(
			authority_accounts,
			account4.clone(),
			Keypair::generate_ed25519(),
			1512,
		),
	];

	let bootnodes = {
		let bootnodes_spec = &specs[0];
		let bootnodes = (
			bootnodes_spec.2.public().into_peer_id(),
			Multiaddr::empty()
				.with(Protocol::Ip4([127, 0, 0, 1].into()))
				.with(Protocol::Tcp(bootnodes_spec.3)),
		);
		let bootnodes =
			std::iter::once((bootnodes, ())).collect::<LinkedHashMap<(PeerId, Multiaddr), ()>>();
		bootnodes
	};

	for spec in &specs {
		info!("address: {}", spec.1.address);
		info!("peer id: {}", spec.2.public().into_peer_id());
	}

	let services = specs
		.iter()
		.map(|x| base::get_service(&x.0, &x.1, x.2.clone(), x.3, bootnodes.clone()))
		.collect::<Vec<_>>();

	let chain = &services[0].0;
	let txpool = &services[0].1;
	let _consensus = &services[0].2;

	// generate block 1
	base::wait_block_execution(&chain, 1).await;

	let block_number = chain.get_confirmed_number().unwrap().unwrap();
	log::info!("block_number: {}", block_number);

	let admin: module::hotstuff::Admin = chain
		.execute_call_with_block_number(
			&block_number,
			None,
			"hotstuff".to_string(),
			"get_admin".to_string(),
			EmptyParams,
		)
		.unwrap()
		.unwrap();
	log::info!("admin: {:?}", admin);
	assert_eq!(
		admin,
		module::hotstuff::Admin {
			threshold: 1,
			members: vec![(account1.address.clone(), 1)],
		}
	);

	// update admin
	let tx1_hash = base::insert_tx(
		&chain,
		&txpool,
		chain
			.build_transaction(
				Some((account1.secret_key.clone(), 0, 10)),
				chain
					.build_call(
						"hotstuff".to_string(),
						"update_admin".to_string(),
						module::hotstuff::UpdateAdminParams {
							admin: module::hotstuff::Admin {
								threshold: 2,
								members: vec![
									(account1.address.clone(), 1),
									(account2.address.clone(), 1),
								],
							},
						},
					)
					.unwrap(),
			)
			.unwrap(),
	)
	.await;
	base::wait_txpool(&txpool, 1).await;

	// generate block 2
	base::wait_block_execution(&chain, 2).await;

	let tx1_receipt = chain.get_receipt(&tx1_hash).unwrap().unwrap();
	let tx1_events = tx1_receipt
		.events
		.into_iter()
		.map(|x| {
			let event = String::from_utf8(x.0).unwrap();
			event
		})
		.collect::<Vec<_>>();
	log::info!("tx1_events: {:x?}", tx1_events);

	let block_number = chain.get_confirmed_number().unwrap().unwrap();
	let admin: module::hotstuff::Admin = chain
		.execute_call_with_block_number(
			&block_number,
			None,
			"hotstuff".to_string(),
			"get_admin".to_string(),
			EmptyParams,
		)
		.unwrap()
		.unwrap();
	log::info!("admin: {:?}", admin);
	assert_eq!(
		admin,
		module::hotstuff::Admin {
			threshold: 2,
			members: vec![(account1.address.clone(), 1), (account2.address.clone(), 1)],
		}
	);

	// update admin
	let tx1_hash = base::insert_tx(
		&chain,
		&txpool,
		chain
			.build_transaction(
				Some((account2.secret_key.clone(), 0, 10)),
				chain
					.build_call(
						"hotstuff".to_string(),
						"update_admin".to_string(),
						module::hotstuff::UpdateAdminParams {
							admin: module::hotstuff::Admin {
								threshold: 1,
								members: vec![(account2.address.clone(), 1)],
							},
						},
					)
					.unwrap(),
			)
			.unwrap(),
	)
	.await;

	let tx2_hash = base::insert_tx(
		&chain,
		&txpool,
		chain
			.build_transaction(
				Some((account1.secret_key.clone(), 0, 10)),
				chain
					.build_call(
						"hotstuff".to_string(),
						"update_admin_vote".to_string(),
						module::hotstuff::UpdateAdminVoteParams { proposal_id: 2 },
					)
					.unwrap(),
			)
			.unwrap(),
	)
	.await;
	base::wait_txpool(&txpool, 2).await;

	// generate block 3
	base::wait_block_execution(&chain, 3).await;

	let tx1_receipt = chain.get_receipt(&tx1_hash).unwrap().unwrap();
	let tx1_events = tx1_receipt
		.events
		.into_iter()
		.map(|x| {
			let event = String::from_utf8(x.0).unwrap();
			event
		})
		.collect::<Vec<_>>();
	log::info!("tx1_events: {:x?}", tx1_events);

	let tx2_receipt = chain.get_receipt(&tx2_hash).unwrap().unwrap();
	log::info!("tx2_result: {:x?}", tx2_receipt.result);
	let tx2_events = tx2_receipt
		.events
		.into_iter()
		.map(|x| {
			let event = String::from_utf8(x.0).unwrap();
			event
		})
		.collect::<Vec<_>>();
	log::info!("tx2_events: {:x?}", tx2_events);

	let block_number = chain.get_confirmed_number().unwrap().unwrap();
	let admin: module::hotstuff::Admin = chain
		.execute_call_with_block_number(
			&block_number,
			None,
			"hotstuff".to_string(),
			"get_admin".to_string(),
			EmptyParams,
		)
		.unwrap()
		.unwrap();
	log::info!("admin: {:?}", admin);
	assert_eq!(
		admin,
		module::hotstuff::Admin {
			threshold: 1,
			members: vec![(account2.address.clone(), 1)],
		}
	);
}

#[tokio::test]
async fn test_hotstuff_update_authorities() {
	let _ = env_logger::try_init();

	let dsa = Arc::new(DsaImpl::Ed25519);
	let address = Arc::new(AddressImpl::Blake2b160);

	let test_accounts = test_accounts(dsa.clone(), address);
	let (account1, account2, account3, account4, account5) = (
		&test_accounts[0],
		&test_accounts[1],
		&test_accounts[2],
		&test_accounts[3],
		&test_accounts[4],
	);

	let authority_accounts = [account1, account2, account3, account4];

	let specs = vec![
		(
			authority_accounts,
			account1.clone(),
			Keypair::generate_ed25519(),
			1513,
		),
		(
			authority_accounts,
			account2.clone(),
			Keypair::generate_ed25519(),
			1514,
		),
		(
			authority_accounts,
			account3.clone(),
			Keypair::generate_ed25519(),
			1515,
		),
		(
			authority_accounts,
			account4.clone(),
			Keypair::generate_ed25519(),
			1516,
		),
		(
			authority_accounts,
			account5.clone(),
			Keypair::generate_ed25519(),
			1517,
		),
	];

	let bootnodes = {
		let bootnodes_spec = &specs[0];
		let bootnodes = (
			bootnodes_spec.2.public().into_peer_id(),
			Multiaddr::empty()
				.with(Protocol::Ip4([127, 0, 0, 1].into()))
				.with(Protocol::Tcp(bootnodes_spec.3)),
		);
		let bootnodes =
			std::iter::once((bootnodes, ())).collect::<LinkedHashMap<(PeerId, Multiaddr), ()>>();
		bootnodes
	};

	for spec in &specs {
		info!("peer id: {}", spec.2.public().into_peer_id());
		info!("address: {}", spec.1.address);
	}

	let services = specs
		.iter()
		.map(|x| base::get_service(&x.0, &x.1, x.2.clone(), x.3, bootnodes.clone()))
		.collect::<Vec<_>>();

	let chain = &services[0].0;
	let txpool = &services[0].1;
	let _consensus = &services[0].2;

	// generate block 1
	base::wait_block_execution(&chain, 1).await;

	let block_number = chain.get_confirmed_number().unwrap().unwrap();
	log::info!("block_number: {}", block_number);

	let authorities: Authorities = chain
		.execute_call_with_block_number(
			&block_number,
			None,
			"hotstuff".to_string(),
			"get_authorities".to_string(),
			EmptyParams,
		)
		.unwrap()
		.unwrap();
	assert_eq!(
		authorities,
		Authorities {
			members: vec![
				(account1.address.clone(), 1),
				(account2.address.clone(), 1),
				(account3.address.clone(), 1),
				(account4.address.clone(), 1),
			]
		}
	);

	let admin: module::hotstuff::Admin = chain
		.execute_call_with_block_number(
			&block_number,
			None,
			"hotstuff".to_string(),
			"get_admin".to_string(),
			EmptyParams,
		)
		.unwrap()
		.unwrap();
	log::info!("admin: {:?}", admin);
	assert_eq!(
		admin,
		module::hotstuff::Admin {
			threshold: 1,
			members: vec![(account1.address.clone(), 1)],
		}
	);

	// update admin
	let tx1_hash = base::insert_tx(
		&chain,
		&txpool,
		chain
			.build_transaction(
				Some((account1.secret_key.clone(), 0, 10)),
				chain
					.build_call(
						"hotstuff".to_string(),
						"update_admin".to_string(),
						module::hotstuff::UpdateAdminParams {
							admin: module::hotstuff::Admin {
								threshold: 2,
								members: vec![
									(account1.address.clone(), 1),
									(account2.address.clone(), 1),
								],
							},
						},
					)
					.unwrap(),
			)
			.unwrap(),
	)
	.await;
	base::wait_txpool(&txpool, 1).await;

	// generate block 2
	base::wait_block_execution(&chain, 2).await;

	let tx1_receipt = chain.get_receipt(&tx1_hash).unwrap().unwrap();
	let tx1_events = tx1_receipt
		.events
		.into_iter()
		.map(|x| {
			let event = String::from_utf8(x.0).unwrap();
			event
		})
		.collect::<Vec<_>>();
	log::info!("tx1_events: {:x?}", tx1_events);

	let block_number = chain.get_confirmed_number().unwrap().unwrap();
	let admin: module::hotstuff::Admin = chain
		.execute_call_with_block_number(
			&block_number,
			None,
			"hotstuff".to_string(),
			"get_admin".to_string(),
			EmptyParams,
		)
		.unwrap()
		.unwrap();
	log::info!("admin: {:?}", admin);
	assert_eq!(
		admin,
		module::hotstuff::Admin {
			threshold: 2,
			members: vec![(account1.address.clone(), 1), (account2.address.clone(), 1)],
		}
	);

	let new_authorities = vec![
		(account2.address.clone(), 1),
		(account3.address.clone(), 1),
		(account4.address.clone(), 1),
		(account5.address.clone(), 1),
	];

	// update authorities
	let tx1_hash = base::insert_tx(
		&chain,
		&txpool,
		chain
			.build_transaction(
				Some((account2.secret_key.clone(), 0, 10)),
				chain
					.build_call(
						"hotstuff".to_string(),
						"update_authorities".to_string(),
						module::hotstuff::UpdateAuthoritiesParams {
							authorities: Authorities {
								members: new_authorities.clone(),
							},
						},
					)
					.unwrap(),
			)
			.unwrap(),
	)
	.await;

	let tx2_hash = base::insert_tx(
		&chain,
		&txpool,
		chain
			.build_transaction(
				Some((account1.secret_key.clone(), 0, 10)),
				chain
					.build_call(
						"hotstuff".to_string(),
						"update_authorities_vote".to_string(),
						module::hotstuff::UpdateAuthoritiesVoteParams { proposal_id: 1 },
					)
					.unwrap(),
			)
			.unwrap(),
	)
	.await;
	base::wait_txpool(&txpool, 2).await;

	// generate block 3
	base::wait_block_execution(&chain, 3).await;

	let tx1_receipt = chain.get_receipt(&tx1_hash).unwrap().unwrap();
	let tx1_events = tx1_receipt
		.events
		.into_iter()
		.map(|x| {
			let event = String::from_utf8(x.0).unwrap();
			event
		})
		.collect::<Vec<_>>();
	log::info!("tx1_events: {:x?}", tx1_events);

	let tx2_receipt = chain.get_receipt(&tx2_hash).unwrap().unwrap();
	log::info!("tx2_result: {:x?}", tx2_receipt.result);
	let tx2_events = tx2_receipt
		.events
		.into_iter()
		.map(|x| {
			let event = String::from_utf8(x.0).unwrap();
			event
		})
		.collect::<Vec<_>>();
	log::info!("tx2_events: {:x?}", tx2_events);

	let block_number = chain.get_confirmed_number().unwrap().unwrap();
	let authorities: Authorities = chain
		.execute_call_with_block_number(
			&block_number,
			None,
			"hotstuff".to_string(),
			"get_authorities".to_string(),
			EmptyParams,
		)
		.unwrap()
		.unwrap();
	assert_eq!(
		authorities,
		Authorities {
			members: new_authorities,
		}
	);

	// generate block 10
	base::wait_block_execution(&chain, 10).await;

	let block_number = chain.get_confirmed_number().unwrap().unwrap();
	log::info!("block_number: {}", block_number);
}
