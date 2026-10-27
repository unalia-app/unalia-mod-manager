#!/usr/bin/env node
// Faux dossier Steam de démo, pour voir Modalia avec de vrais jeux sans
// avoir Steam : manifestes, dossiers de jeu vides avec les signatures de
// moteur, et vraies jaquettes téléchargées depuis le CDN public de Steam.
//
// Tout est écrit dans `.demo/` (ignoré par Git) : aucun asset officiel
// n'entre dans le repo (SPEC §4).
//
//   pnpm demo          prépare la démo puis lance l'appli dessus
//   pnpm demo --setup  prépare seulement

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, readdir, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = join(repo, ".demo", "steam");

/**
 * [appid, nom, moteur]. Pas de Silksong ni de Cyberpunk 2077 : ils sont
 * ajoutés à la main depuis leur vrai dossier, pour les vidéos.
 */
const GAMES = [
	[367520, "Hollow Knight", "unity"],
	[1966720, "Lethal Company", "unity"],
	[3241660, "R.E.P.O.", "unity"],
	[892970, "Valheim", "unity"],
	[632360, "Risk of Rain 2", "unity"],
	[2881650, "Content Warning", "unity"],
	[3527290, "PEAK", "unity"],
	[1313140, "Cult of the Lamb", "unity"],
	[1366540, "Dyson Sphere Program", "unity"],
	[1092790, "Inscryption", "unity"],
	[268910, "Cuphead", "unity"],
	[264710, "Subnautica", "unity"],
	[3164500, "Schedule I", "unity"],
	[3146520, "WEBFISHING", "unknown"],
	[1557740, "ROUNDS", "unity"],
	[794260, "Outward", "unity"],
	[526870, "Satisfactory", "unreal"],
	[548430, "Deep Rock Galactic", "unreal"],
	[1623730, "Palworld", "unreal"],
	[1145350, "Hades II", "unknown"],
	[1086940, "Baldur's Gate 3", "unknown"],
	[730, "Counter-Strike 2", "unknown"],
];

const ARTWORK = ["library_600x900.jpg", "library_hero.jpg"];
const CDN = "https://cdn.cloudflare.steamstatic.com/steam/apps";

function vdfString(s) {
	return `"${s.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`;
}

function installDir(name) {
	return name.replace(/[^\p{L}\p{N} .'-]/gu, "").trim();
}

async function touch(path) {
	await mkdir(dirname(path), { recursive: true });
	if (!existsSync(path)) await writeFile(path, "");
}

/** Fichiers vides qui portent la signature du moteur (voir detect/engine.rs). */
async function writeGameFiles(dir, name, engine) {
	const exe = name.replace(/[^\p{L}\p{N}]/gu, "");
	await touch(join(dir, `${exe}.exe`));
	if (engine === "unity") {
		await touch(join(dir, "UnityPlayer.dll"));
		await touch(join(dir, `${exe}_Data`, "globalgamemanagers"));
	} else if (engine === "unreal") {
		await touch(join(dir, "Engine", "Binaries", "readme.txt"));
		await touch(join(dir, exe, "Content", "Paks", `${exe}-Windows.pak`));
	}
}

async function download(appId, file) {
	const target = join(root, "appcache", "librarycache", String(appId), file);
	if (existsSync(target)) return "cache";
	const response = await fetch(`${CDN}/${appId}/${file}`);
	const type = response.headers.get("content-type") ?? "";
	if (!response.ok || !type.startsWith("image/")) return "absent";
	await mkdir(dirname(target), { recursive: true });
	await writeFile(target, Buffer.from(await response.arrayBuffer()));
	return "ok";
}

async function setup() {
	const steamapps = join(root, "steamapps");
	await mkdir(steamapps, { recursive: true });

	await writeFile(
		join(steamapps, "libraryfolders.vdf"),
		`"libraryfolders"\n{\n\t"0"\n\t{\n\t\t"path"\t\t${vdfString(root)}\n\t}\n}\n`,
	);

	// Un jeu retiré de la liste quitte aussi la bibliothèque de démo.
	const kept = new Set(GAMES.map(([appId]) => `appmanifest_${appId}.acf`));
	for (const file of await readdir(steamapps)) {
		if (file.startsWith("appmanifest_") && !kept.has(file)) await rm(join(steamapps, file));
	}

	for (const [appId, name, engine] of GAMES) {
		const dir = installDir(name);
		await writeFile(
			join(steamapps, `appmanifest_${appId}.acf`),
			`"AppState"\n{\n\t"appid"\t\t"${appId}"\n\t"name"\t\t${vdfString(name)}\n\t"StateFlags"\t\t"4"\n\t"installdir"\t\t${vdfString(dir)}\n}\n`,
		);
		await writeGameFiles(join(steamapps, "common", dir), name, engine);
	}

	// Quelques téléchargements en parallèle, pas plus : on reste poli avec le CDN.
	const jobs = GAMES.flatMap(([appId, name]) => ARTWORK.map((file) => ({ appId, name, file })));
	const missing = [];
	let done = 0;
	const worker = async () => {
		for (let job = jobs.shift(); job; job = jobs.shift()) {
			const status = await download(job.appId, job.file).catch(() => "absent");
			if (status === "absent") missing.push(`${job.name} (${job.file})`);
			done++;
			process.stdout.write(`\r  Images : ${done}/${GAMES.length * ARTWORK.length}`);
		}
	};
	await Promise.all(Array.from({ length: 6 }, worker));
	process.stdout.write("\n");

	console.log(`  ${GAMES.length} jeux prêts dans ${root}`);
	if (missing.length) console.log(`  Images introuvables sur le CDN : ${missing.join(", ")}`);
}

console.log("Préparation du Steam de démo…");
await setup();

if (!process.argv.includes("--setup")) {
	console.log("Lancement de Modalia sur la démo…");
	const child = spawn("pnpm", ["--filter", "@modalia/desktop", "tauri", "dev"], {
		cwd: repo,
		stdio: "inherit",
		env: { ...process.env, MODALIA_STEAM_ROOT: root },
	});
	child.on("exit", (code) => process.exit(code ?? 0));
}
