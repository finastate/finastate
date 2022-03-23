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

#![feature(test)]

extern crate test;

use std::hint::black_box;
use test::Bencher;

use crypto::dsa::{Dsa, DsaImpl, KeyPair, Verifier};
use crypto::DsaLength;
use crypto_dylib_samples_dsa::Ed25519;

#[bench]
fn bench_sign_ed25519_ring(b: &mut Bencher) {
	let secret: [u8; 32] = [
		184, 80, 22, 77, 31, 238, 200, 105, 138, 204, 163, 41, 148, 124, 152, 133, 189, 29, 148, 3,
		77, 47, 187, 230, 8, 5, 152, 173, 190, 21, 178, 152,
	];
	bench_sign(&DsaImpl::Ed25519, &secret, b);
}

#[bench]
fn bench_sign_sm2(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_sign(&DsaImpl::SM2, &secret, b);
}

#[bench]
fn bench_sign_ed25519_dylib(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_sign(&dsa_from_dylib(), &secret, b);
}

#[bench]
fn bench_sign_ed25519_dalek(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_sign(&Ed25519, &secret, b);
}

#[bench]
fn bench_sign_full_ed25519_ring(b: &mut Bencher) {
	let secret: [u8; 32] = [
		184, 80, 22, 77, 31, 238, 200, 105, 138, 204, 163, 41, 148, 124, 152, 133, 189, 29, 148, 3,
		77, 47, 187, 230, 8, 5, 152, 173, 190, 21, 178, 152,
	];
	bench_sign_full(&DsaImpl::Ed25519, &secret, b);
}

#[bench]
fn bench_sign_full_sm2(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_sign_full(&DsaImpl::SM2, &secret, b);
}

#[bench]
fn bench_sign_full_ed25519_dylib(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];

	bench_sign_full(&dsa_from_dylib(), &secret, b);
}

#[bench]
fn bench_sign_full_ed25519_dalek(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_sign_full(&Ed25519, &secret, b);
}

#[bench]
fn bench_verify_ed25519_ring(b: &mut Bencher) {
	let secret: [u8; 32] = [
		184, 80, 22, 77, 31, 238, 200, 105, 138, 204, 163, 41, 148, 124, 152, 133, 189, 29, 148, 3,
		77, 47, 187, 230, 8, 5, 152, 173, 190, 21, 178, 152,
	];
	bench_verify(&DsaImpl::Ed25519, &secret, b);
}

#[bench]
fn bench_verify_sm2(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_verify(&DsaImpl::SM2, &secret, b);
}

#[bench]
fn bench_verify_ed25519_dylib(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_verify(&dsa_from_dylib(), &secret, b);
}

#[bench]
fn bench_verify_ed25519_dalek(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_verify(&Ed25519, &secret, b);
}

#[bench]
fn bench_verify_full_ed25519_ring(b: &mut Bencher) {
	let secret: [u8; 32] = [
		184, 80, 22, 77, 31, 238, 200, 105, 138, 204, 163, 41, 148, 124, 152, 133, 189, 29, 148, 3,
		77, 47, 187, 230, 8, 5, 152, 173, 190, 21, 178, 152,
	];
	bench_verify_full(&DsaImpl::Ed25519, &secret, b);
}

#[bench]
fn bench_verify_full_sm2(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_verify_full(&DsaImpl::SM2, &secret, b);
}

#[bench]
fn bench_verify_full_ed25519_dylib(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_verify_full(&dsa_from_dylib(), &secret, b);
}

#[bench]
fn bench_verify_full_ed25519_dalek(b: &mut Bencher) {
	let secret: [u8; 32] = [
		128, 166, 19, 115, 227, 79, 114, 21, 254, 206, 184, 221, 6, 187, 55, 49, 234, 54, 47, 245,
		53, 90, 114, 38, 212, 225, 45, 7, 106, 126, 181, 136,
	];
	bench_verify_full(&Ed25519, &secret, b);
}

fn bench_sign<D: Dsa>(dsa: &D, secret_key: &[u8], b: &mut Bencher) {
	let key_pair = dsa.key_pair_from_secret_key(&secret_key).unwrap();

	let message = [1u8; 256];

	let mut out = [0u8; 64];

	b.iter(|| black_box(key_pair.sign(&message, &mut out)));
}

fn bench_sign_full<D: Dsa>(dsa: &D, secret_key: &[u8], b: &mut Bencher) {
	let message = [1u8; 256];

	let mut out = [0u8; 64];

	let mut run = || {
		let key_pair = dsa.key_pair_from_secret_key(&secret_key).unwrap();
		key_pair.sign(&message, &mut out)
	};

	b.iter(|| black_box(run()));
}

fn bench_verify<D: Dsa>(dsa: &D, secret_key: &[u8], b: &mut Bencher) {
	let key_pair = dsa.key_pair_from_secret_key(&secret_key).unwrap();

	let message = [1u8; 256];

	let mut signature = [0u8; 64];

	key_pair.sign(&message, &mut signature);

	let length = dsa.length();

	let verifier = match length.into() {
		(_, 65, _) => {
			let mut public_key = [0u8; 65];
			key_pair.public_key(&mut public_key);
			dsa.verifier_from_public_key(&public_key).unwrap()
		}
		(_, 32, _) => {
			let mut public_key = [0u8; 32];
			key_pair.public_key(&mut public_key);
			dsa.verifier_from_public_key(&public_key).unwrap()
		}
		_ => unreachable!(),
	};

	verifier.verify(&message, &signature).unwrap();

	b.iter(|| black_box(verifier.verify(&message, &signature)));
}

fn bench_verify_full<D: Dsa>(dsa: &D, secret_key: &[u8], b: &mut Bencher) {
	let key_pair = dsa.key_pair_from_secret_key(&secret_key).unwrap();

	let message = [1u8; 256];

	let mut signature = [0u8; 64];

	key_pair.sign(&message, &mut signature);

	let length = dsa.length().into();

	let run = move || {
		let verifier = match &length {
			DsaLength::DsaLength32_65_64 => {
				let mut public_key = [0u8; 65];
				key_pair.public_key(&mut public_key);
				dsa.verifier_from_public_key(&public_key).unwrap()
			}
			DsaLength::DsaLength32_32_64 => {
				let mut public_key = [0u8; 32];
				key_pair.public_key(&mut public_key);
				dsa.verifier_from_public_key(&public_key).unwrap()
			}
		};
		verifier.verify(&message, &signature)
	};

	b.iter(|| black_box(run()));
}

fn dsa_from_dylib() -> DsaImpl {
	use std::str::FromStr;

	let path = utils_test::get_dylib("crypto_dylib_samples_dsa");

	assert!(
		path.exists(),
		"Should build first to make exist: {:?}",
		path
	);

	let path = path.to_string_lossy();
	let dsa = DsaImpl::from_str(&path).unwrap();
	dsa
}

#[bench]
fn bench_vec_copy(b: &mut Bencher) {
	let source = vec![1u8; 64];

	let run = || {
		let _clone = source.clone();
	};
	b.iter(|| black_box(run()));
}
