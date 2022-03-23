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

use rust_crypto::blake2b;

use crate::hash::Hash;
use crate::HashLength;

pub struct Blake2b160;

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

pub struct Blake2b256;

impl Hash for Blake2b256 {
	fn name(&self) -> String {
		"blake2b_256".to_string()
	}
	fn length(&self) -> HashLength {
		HashLength::HashLength32
	}
	fn hash(&self, out: &mut [u8], data: &[u8]) {
		let len: usize = self.length().into();
		assert_eq!(out.len(), len);
		blake2b::Blake2b::blake2b(out, data, &[]);
	}
}

pub struct Blake2b512;

impl Hash for Blake2b512 {
	fn name(&self) -> String {
		"blake2b_512".to_string()
	}
	fn length(&self) -> HashLength {
		HashLength::HashLength64
	}
	fn hash(&self, out: &mut [u8], data: &[u8]) {
		let len: usize = self.length().into();
		assert_eq!(out.len(), len);
		blake2b::Blake2b::blake2b(out, data, &[]);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_blake2b_256() {
		let data = [1u8, 2u8, 3u8];
		let mut out = [0u8; 32];
		Blake2b256.hash(&mut out, &data);

		assert_eq!(
			out,
			[
				17, 192, 231, 155, 113, 195, 151, 108, 205, 12, 2, 209, 49, 14, 37, 22, 192, 142,
				220, 157, 139, 111, 87, 204, 214, 128, 214, 58, 77, 142, 114, 218
			]
		);
	}

	#[test]
	fn test_blake2b_160() {
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

	#[test]
	fn test_blake2b_512() {
		let data = [1u8, 2u8, 3u8];
		let mut out = [0u8; 64];
		Blake2b512.hash(&mut out, &data);

		let expect: [u8; 64] = [
			207, 148, 246, 214, 5, 101, 126, 144, 197, 67, 176, 201, 25, 7, 12, 218, 175, 114, 9,
			197, 225, 234, 88, 172, 184, 243, 86, 143, 162, 17, 66, 104, 220, 154, 195, 186, 254,
			18, 175, 39, 125, 40, 111, 206, 125, 197, 155, 124, 12, 52, 137, 115, 196, 233, 218,
			203, 231, 148, 133, 229, 106, 194, 167, 2,
		];

		assert_eq!(out.to_vec(), expect.to_vec(),);
	}
}
