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

#[macro_use]
extern crate crypto;

use rust_crypto::blake2b;

use crypto::hash::Hash;
use crypto::HashLength;

pub struct Blake2b160;

/// A Blake2b160 implementation for sample
impl Hash for Blake2b160 {
	fn name(&self) -> String {
		"blake2b_160".to_string()
	}
	fn length(&self) -> HashLength {
		HashLength::HashLength20
	}
	fn hash(&self, out: &mut [u8], data: &[u8]) {
		let len: usize = self.length().into();
		assert_eq!(out.len(), len);
		blake2b::Blake2b::blake2b(out, data, &[]);
	}
}

declare_hash_custom_lib!(Blake2b160);

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test() {
		let data = [1u8, 2u8, 3u8];
		let mut out = [0u8; 20];
		Blake2b160.hash(&mut out, &data);

		assert_eq!(
			out,
			[
				197, 117, 145, 134, 122, 108, 242, 5, 233, 74, 212, 142, 167, 139, 236, 142, 103,
				194, 14, 98
			]
		);
	}
}
