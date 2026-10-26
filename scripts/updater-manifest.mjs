#!/usr/bin/env node
// Manifeste lu par l'updater de l'appli (`latest.json`), à joindre à chaque
// release sous ce nom : l'appli le cherche au lien permanent de la dernière
// release (`plugins.updater.endpoints` dans `tauri.conf.json`).
//
// À lancer après `tauri build`, avec la clé privée dans
// `TAURI_SIGNING_PRIVATE_KEY` (sinon pas de `.sig`) :
//
//   node scripts/updater-manifest.mjs --url <adresse publique de l'installeur>
//
// Les notes de version viennent de la section de `CHANGELOG.md` qui porte
// le numéro de la version.

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

const manifest = {
	version,
	notes: await changelogNotes(version),
	pub_date: new Date().toISOString(),
	platforms: {
		"windows-x86_64": { signature: signature.trim(), url: values.url },
	},
};
await writeFile(values.out, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`${values.out} : ${installer}, version ${version}`);

/** Corps de la section `## [<version>]` du changelog, sans son titre. */
async function changelogNotes(version) {
	const text = await readFile(join(repo, "CHANGELOG.md"), "utf8");
	const sections = text.split(/^## /m);
	const section = sections.find((s) => s.startsWith(`[${version}]`));
	return section ? section.slice(section.indexOf("\n") + 1).trim() : "";
}
