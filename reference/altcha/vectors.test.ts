import { writeFileSync } from 'node:fs';
import { test } from 'vitest';
import { createChallenge, solveChallenge } from '../src/pow';
import { bufferToHex, canonicalJSON, hexToBuffer, hmac } from '../src/helpers';
import { deriveKey as sha } from '../src/algorithms/sha';
import { deriveKey as pbkdf2 } from '../src/algorithms/pbkdf2';
import { deriveKey as scrypt } from '../src/algorithms/scrypt';
import { deriveKey as argon2id } from '../src/algorithms/argon2id';
import { ObfuscationPlugin } from '../src/plugins/obfuscation.plugin';
import { HmacAlgorithm } from '../src/types';

const NONCE = '000102030405060708090a0b0c0d0e0f';
const SALT = 'aabbccddeeff00112233445566778899';

function password(nonce: string, counter: number, mode: 'uint32' | 'string') {
	const nonceBuf = hexToBuffer(nonce);
	if (mode === 'string') {
		const suffix = new TextEncoder().encode(String(counter));
		const out = new Uint8Array(nonceBuf.length + suffix.length);
		out.set(nonceBuf, 0);
		out.set(suffix, nonceBuf.length);
		return out;
	}
	const out = new Uint8Array(nonceBuf.length + 4);
	out.set(nonceBuf, 0);
	new DataView(out.buffer).setUint32(nonceBuf.length, counter, false);
	return out;
}

test('vectors', async () => {
	const out: Record<string, unknown> = {};

	const shaKey = await sha(
		{ algorithm: 'SHA-256', cost: 1, keyLength: 32 } as any,
		hexToBuffer(SALT),
		password(NONCE, 42, 'uint32')
	);
	out.sha256 = bufferToHex(shaKey.derivedKey);

	const shaChain = await sha(
		{ algorithm: 'SHA-512', cost: 7, keyLength: 64 } as any,
		hexToBuffer(SALT),
		password(NONCE, 3, 'uint32')
	);
	out.sha512_cost7 = bufferToHex(shaChain.derivedKey);

	const shaTruncated = await sha(
		{ algorithm: 'SHA-256', cost: 3, keyLength: 16 } as any,
		hexToBuffer(SALT),
		password(NONCE, 5, 'uint32')
	);
	out.sha256_cost3_len16 = bufferToHex(shaTruncated.derivedKey);

	const v1Salt = 'saltysalt?expires=1700000000';
	const v1Password = new TextEncoder().encode(v1Salt + '12345');
	const v1 = await sha(
		{ algorithm: 'SHA-256', cost: 1, keyLength: 32 } as any,
		new Uint8Array(0),
		v1Password
	);
	out.v1_sha256 = bufferToHex(v1.derivedKey);
	out.v1_signature = bufferToHex(
		await hmac(HmacAlgorithm.SHA_256, bufferToHex(v1.derivedKey), 'signature.secret')
	);

	const pb = await pbkdf2(
		{ algorithm: 'PBKDF2/SHA-256', cost: 5000, keyLength: 32 } as any,
		hexToBuffer(SALT),
		password(NONCE, 11, 'uint32')
	);
	out.pbkdf2_sha256_5000 = bufferToHex(pb.derivedKey);

	const pb512 = await pbkdf2(
		{ algorithm: 'PBKDF2/SHA-512', cost: 1000, keyLength: 16 } as any,
		hexToBuffer(SALT),
		password(NONCE, 2, 'uint32')
	);
	out.pbkdf2_sha512_1000_len16 = bufferToHex(pb512.derivedKey);

	const sc = await scrypt(
		{ algorithm: 'SCRYPT', cost: 1024, keyLength: 32, memoryCost: 8, parallelism: 1 } as any,
		hexToBuffer(SALT),
		password(NONCE, 4, 'uint32')
	);
	out.scrypt_1024_8_1 = bufferToHex(sc.derivedKey);

	const ar = await argon2id(
		{ algorithm: 'ARGON2ID', cost: 2, keyLength: 32, memoryCost: 1024, parallelism: 1 } as any,
		hexToBuffer(SALT),
		password(NONCE, 9, 'uint32')
	);
	out.argon2id_t2_m1024_p1 = bufferToHex(ar.derivedKey);

	const parameters = {
		algorithm: 'SHA-256',
		cost: 100,
		expiresAt: 1700000000,
		keyLength: 32,
		keyPrefix: 'ab'.repeat(16),
		nonce: NONCE,
		salt: SALT
	};
	out.canonical = canonicalJSON(parameters);
	out.canonical_signature = bufferToHex(
		await hmac(HmacAlgorithm.SHA_256, canonicalJSON(parameters), 'signature.secret')
	);

	const challenge = await createChallenge({
		algorithm: 'SHA-256',
		cost: 10,
		counter: 37,
		deriveKey: sha,
		hmacSignatureSecret: 'signature.secret',
		keyLength: 32
	});
	out.challenge = challenge;
	const solution = await solveChallenge({ challenge, deriveKey: sha });
	out.challenge_solution = { counter: solution?.counter, derivedKey: solution?.derivedKey };

	out.obfuscated = await ObfuscationPlugin.obfuscate('mailto:hidden@example.com', {
		cost: 200,
		counterMin: 5,
		counterMax: 9
	});

	const verificationData =
		'classification=GOOD&email=user%40example.com&expire=4102444800&fields=email%2Cname&fieldsHash=' +
		bufferToHex(
			new Uint8Array(
				await crypto.subtle.digest('SHA-256', new TextEncoder().encode('user@example.com\nAda'))
			)
		) +
		'&score=1.4&time=1700000000&verified=true';
	out.server_signature_payload = btoa(
		JSON.stringify({
			algorithm: 'SHA-256',
			verificationData,
			signature: bufferToHex(
				await hmac(
					HmacAlgorithm.SHA_256,
					new Uint8Array(
						await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verificationData))
					),
					'signature.secret'
				)
			),
			verified: true
		})
	);

	writeFileSync('vectors.json', JSON.stringify(out, null, 2));
});
