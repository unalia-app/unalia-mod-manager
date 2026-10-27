#!/usr/bin/env node
// Manifeste lu par l'updater de l'appli (`latest.json`), à joindre à chaque
// release sous ce nom : l'appli le cherche au lien permanent de la dernière
// release (`plugins.updater.endpoints` dans `tauri.conf.json`).
//
// À lancer après `tauri build`, avec la clé privée dans
// `TAURI_SIGNING_PRIVATE_KEY` (sinon pas de `.sig`) :
//
//   node scripts/updater-manifest.mjs --url <adresse publique de l'installeur>
//                                     [--out latest.json] [--notes notes.md]
//
// Les notes de version viennent de la section de `CHANGELOG.md` qui porte
// le numéro de la version. `--notes` les écrit aussi à part, pour la
// description de la release GitHub.

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
	console.error("--url <https://...> requis : adresse publique de l'installeur");
	process.exit(1);
}

const config = JSON.parse(await readFile(join(repo, "apps", "desktop", "src-tauri", "tauri.conf.json"), "utf8"));
const version = config.version;

const installer = (await readdir(nsis)).find((f) => f.endsWith("-setup.exe") && f.includes(`_${version}_`));
if (!installer) {
	console.error(`Aucun installeur ${version} dans ${nsis} : lancer \`tauri build\` d'abord.`);
	process.exit(1);
}
const signature = await readFile(join(nsis, `${installer}.sig`), "utf8").catch(() => {
	console.error(`${installer}.sig introuvable : TAURI_SIGNING_PRIVATE_KEY était-elle définie pendant le build ?`);
	process.exit(1);
});
// `tauri build` ne fait qu'avertir quand la clé privée n'est pas celle de
// `plugins.updater.pubkey` : une mise à jour publiée ainsi serait refusée
// par toutes les applis installées.
if (!signedBy(await readFile(join(nsis, installer)), signature, config.plugins.updater.pubkey)) {
	console.error(`${installer}.sig ne correspond pas à la clé publique de tauri.conf.json : mauvaise clé privée, ou installeur modifié après sa signature.`);
	process.exit(1);
}

const notes = await changelogNotes(version);
if (!notes) {
	console.error(`Section ## [${version}] vide ou absente de CHANGELOG.md : les notes de version en viennent.`);
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
console.log(`${values.out} : ${installer}, version ${version}`);

/**
 * Vérifie une signature minisign, au format de Tauri : `.sig` et clé
 * publique sont le texte minisign encodé en base64. Ligne 2 de la clé :
 * `Ed`, identifiant (8 octets), clé Ed25519 (32). Ligne 2 de la signature :
 * algorithme (`ED` : sur l'empreinte BLAKE2b-512 du fichier), identifiant,
 * signature (64).
 */
function signedBy(file, sig, pubkey) {
	const line = (b64) => Buffer.from(Buffer.from(b64, "base64").toString("utf8").split("\n")[1], "base64");
	const key = line(pubkey);
	const signature = line(sig.trim());
	if (!key.subarray(2, 10).equals(signature.subarray(2, 10))) return false;
	const data = signature.subarray(0, 2).toString() === "ED" ? createHash("blake2b512").update(file).digest() : file;
	// Clé brute Ed25519 dans son enveloppe SPKI (DER), seule forme qu'accepte Node.
	const spki = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), key.subarray(10, 42)]);
	return verify(null, data, createPublicKey({ key: spki, format: "der", type: "spki" }), signature.subarray(10, 74));
}

/** Corps de la section `## [<version>]` du changelog, sans son titre. */
async function changelogNotes(version) {
	// Fins de ligne Unix : un checkout Windows (autocrlf) donne des CRLF.
	const text = (await readFile(join(repo, "CHANGELOG.md"), "utf8")).replace(/\r\n/g, "\n");
	const sections = text.split(/^## /m);
	const section = sections.find((s) => s.startsWith(`[${version}]`));
	return section ? section.slice(section.indexOf("\n") + 1).trim() : "";
}
