#!/usr/bin/env node
// Manifest read by the app updater (`latest.json`), to attach to each
// release under this name: the app looks for it at the permanent link of
// the latest release (`plugins.updater.endpoints` in `tauri.conf.json`).
//
// Run it after `tauri build`, with the private key in
// `TAURI_SIGNING_PRIVATE_KEY` (else no `.sig`):
//
//   node scripts/updater-manifest.mjs --url <public URL of the installer>
//                                     [--out latest.json] [--notes notes.md]
//
// The release notes come from the `CHANGELOG.md` section that carries the
// version number. `--notes` also writes them separately, for the GitHub
// release description.

import { createHash, createPublicKey, verify } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const nsis = join(repo, "target", "release", "bundle", "nsis");

const { values } = parseArgs({
	options: {
		url: { type: "string" },
		out: { type: "string", default: join(nsis, "latest.json") },
		notes: { type: "string" },
	},
});
if (!values.url?.startsWith("https://")) {
	console.error("--url <https://...> required: public URL of the installer");
	process.exit(1);
}

const config = JSON.parse(await readFile(join(repo, "apps", "desktop", "src-tauri", "tauri.conf.json"), "utf8"));
const version = config.version;

const installer = (await readdir(nsis)).find((f) => f.endsWith("-setup.exe") && f.includes(`_${version}_`));
if (!installer) {
	console.error(`No ${version} installer in ${nsis}: run \`tauri build\` first.`);
	process.exit(1);
}
const signature = await readFile(join(nsis, `${installer}.sig`), "utf8").catch(() => {
	console.error(`${installer}.sig not found: was TAURI_SIGNING_PRIVATE_KEY set during the build?`);
	process.exit(1);
});
// `tauri build` only warns when the private key isn't the one of
// `plugins.updater.pubkey`: an update published that way would be refused
// by every installed app.
if (!signedBy(await readFile(join(nsis, installer)), signature, config.plugins.updater.pubkey)) {
	console.error(`${installer}.sig doesn't match the public key of tauri.conf.json: wrong private key, or installer modified after signing.`);
	process.exit(1);
}

const notes = await changelogNotes(version);
if (!notes) {
	console.error(`Section ## [${version}] empty or missing from CHANGELOG.md: the release notes come from it.`);
	process.exit(1);
}
if (values.notes) await writeFile(values.notes, `${notes}\n`);

const manifest = {
	version,
	notes,
	pub_date: new Date().toISOString(),
	platforms: {
		"windows-x86_64": { signature: signature.trim(), url: values.url },
	},
};
await writeFile(values.out, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`${values.out}: ${installer}, version ${version}`);

/**
 * Checks a minisign signature, in Tauri's format: `.sig` and public key
 * are the minisign text encoded in base64. Line 2 of the key: `Ed`, id (8
 * bytes), Ed25519 key (32). Line 2 of the signature: algorithm (`ED`: over
 * the BLAKE2b-512 hash of the file), id, signature (64).
 */
function signedBy(file, sig, pubkey) {
	const line = (b64) => Buffer.from(Buffer.from(b64, "base64").toString("utf8").split("\n")[1], "base64");
	const key = line(pubkey);
	const signature = line(sig.trim());
	if (!key.subarray(2, 10).equals(signature.subarray(2, 10))) return false;
	const data = signature.subarray(0, 2).toString() === "ED" ? createHash("blake2b512").update(file).digest() : file;
	// Raw Ed25519 key in its SPKI (DER) envelope, the only form Node accepts.
	const spki = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), key.subarray(10, 42)]);
	return verify(null, data, createPublicKey({ key: spki, format: "der", type: "spki" }), signature.subarray(10, 74));
}

/** Body of the changelog's `## [<version>]` section, without its title. */
async function changelogNotes(version) {
	// Unix line endings: a Windows checkout (autocrlf) gives CRLF.
	const text = (await readFile(join(repo, "CHANGELOG.md"), "utf8")).replace(/\r\n/g, "\n");
	const sections = text.split(/^## /m);
	const section = sections.find((s) => s.startsWith(`[${version}]`));
	return section ? section.slice(section.indexOf("\n") + 1).trim() : "";
}
