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

use std::marker::PhantomData;
use std::sync::Arc;

use fixed_hash::construct_fixed_hash;
use hash_db::Hasher;
use memory_db::PrefixedKey;
use mut_static::MutStatic;
use reference_trie::ReferenceNodeCodec;
use trie_db::{DBValue, TrieLayout};

use crypto::hash::{Hash, HashImpl};
use crypto::HashLength;
use lazy_static::lazy_static;
use primitives::errors::CommonResult;

use crate::errors;

lazy_static! {
	pub static ref HASH_IMPL_20: MutStatic<Arc<HashImpl>> = MutStatic::new();
	pub static ref HASH_IMPL_32: MutStatic<Arc<HashImpl>> = MutStatic::new();
	pub static ref HASH_IMPL_64: MutStatic<Arc<HashImpl>> = MutStatic::new();
}

pub struct TrieHasher20;

pub struct TrieHasher32;

pub struct TrieHasher64;

/// should call load before using TrieHasher20/TrieHasher32/TrieHasher64
pub fn load_hasher(hash_impl: Arc<HashImpl>) -> CommonResult<()> {
	let length = hash_impl.length();
	match length {
		HashLength::HashLength20 => {
			let name = hash_impl.name();
			match HASH_IMPL_20.set(hash_impl) {
				Ok(_) => (),
				Err(mut_static::Error(mut_static::ErrorKind::StaticIsAlreadySet, _))
					if HASH_IMPL_20
						.read()
						.map_err(|_| errors::ErrorKind::LoadHasherFail)?
						.name() == name => {}
				_ => {
					return Err(errors::ErrorKind::LoadHasherConflict(
						HASH_IMPL_32
							.read()
							.map_err(|_| errors::ErrorKind::LoadHasherFail)?
							.name(),
						name,
					)
					.into());
				}
			}
		}
		HashLength::HashLength32 => {
			let name = hash_impl.name();
			match HASH_IMPL_32.set(hash_impl) {
				Ok(_) => (),
				Err(mut_static::Error(mut_static::ErrorKind::StaticIsAlreadySet, _))
					if HASH_IMPL_32
						.read()
						.map_err(|_| errors::ErrorKind::LoadHasherFail)?
						.name() == name => {}
				_ => {
					return Err(errors::ErrorKind::LoadHasherConflict(
						HASH_IMPL_32
							.read()
							.map_err(|_| errors::ErrorKind::LoadHasherFail)?
							.name(),
						name,
					)
					.into());
				}
			}
		}
		HashLength::HashLength64 => {
			let name = hash_impl.name();
			match HASH_IMPL_64.set(hash_impl) {
				Ok(_) => (),
				Err(mut_static::Error(mut_static::ErrorKind::StaticIsAlreadySet, _))
					if HASH_IMPL_64
						.read()
						.map_err(|_| errors::ErrorKind::LoadHasherFail)?
						.name() == name => {}
				_ => {
					return Err(errors::ErrorKind::LoadHasherConflict(
						HASH_IMPL_32
							.read()
							.map_err(|_| errors::ErrorKind::LoadHasherFail)?
							.name(),
						name,
					)
					.into());
				}
			}
		}
	}
	Ok(())
}

pub type DefaultTrieDB<'a, H> = trie_db::TrieDB<'a, DefaultTrieLayout<H>>;

pub type DefaultTrieDBMut<'a, H> = trie_db::TrieDBMut<'a, DefaultTrieLayout<H>>;

pub type DefaultMemoryDB<H> = memory_db::MemoryDB<H, PrefixedKey<H>, DBValue>;

pub struct DefaultTrieLayout<H>(PhantomData<H>);

impl<H: Hasher> TrieLayout for DefaultTrieLayout<H> {
	const USE_EXTENSION: bool = true;
	type Hash = H;
	type Codec = ReferenceNodeCodec<H>;
}

impl Hasher for TrieHasher20 {
	type Out = [u8; 20];
	type StdHasher = DummyStdHasher;
	const LENGTH: usize = 20;

	fn hash(x: &[u8]) -> Self::Out {
		let hasher = HASH_IMPL_20.read().expect("Should load_hasher first");
		let mut out = [0u8; 20];
		hasher.hash(&mut out, x);
		out
	}
}

impl Hasher for TrieHasher32 {
	type Out = [u8; 32];
	type StdHasher = DummyStdHasher;
	const LENGTH: usize = 32;

	fn hash(x: &[u8]) -> Self::Out {
		let hasher = HASH_IMPL_32.read().expect("Should load_hasher first");
		let mut out = [0u8; 32];
		hasher.hash(&mut out, x);
		out
	}
}

construct_fixed_hash! {
	pub struct H512(64);
}

impl Hasher for TrieHasher64 {
	type Out = H512;
	type StdHasher = DummyStdHasher;
	const LENGTH: usize = 64;

	fn hash(x: &[u8]) -> Self::Out {
		let hasher = HASH_IMPL_64.read().expect("Should load_hasher first");
		let mut out = [0u8; 64];
		hasher.hash(&mut out, x);
		H512::from(out)
	}
}

#[derive(Default)]
pub struct DummyStdHasher;

impl std::hash::Hasher for DummyStdHasher {
	fn finish(&self) -> u64 {
		0
	}
	fn write(&mut self, _bytes: &[u8]) {}
}
