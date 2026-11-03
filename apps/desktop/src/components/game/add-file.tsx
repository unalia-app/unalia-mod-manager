"use client";

import { Button } from "@modalia/ui";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { FileArchive } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { toast } from "@/components/toaster";
import { splitError } from "@/lib/engine";
import { installArchive } from "@/lib/installs";

const EXTENSIONS = ["zip", "7z", "rar"];

function installAll(appId: number, paths: string[]) {
	const archives = paths.filter((p) => EXTENSIONS.some((e) => p.toLowerCase().endsWith(`.${e}`)));
	if (archives.length < paths.length) {
		toast({ title: "Only .zip, .7z and .rar files can be installed", tone: "error" });
	}
	for (const path of archives) {
		installArchive(appId, path).catch((e: unknown) => {
			const { message, details } = splitError(e);
			toast({ title: message, details: details ?? undefined, tone: "error" });
		});
	}
}

/** Picks mod archives already on the computer, downloaded by hand. */
export function AddFileButton({ appId, variant = "outline" }: { appId: number; variant?: "outline" | "secondary" }) {
	const pick = async () => {
		const paths = await open({
			multiple: true,
			title: "Pick your mod's file",
			filters: [{ name: "Mod archives", extensions: EXTENSIONS }],
		});
		if (paths) installAll(appId, paths);
	};

	return (
		<Button size="sm" variant={variant} leftIcon={<FileArchive className="size-3.5" />} onClick={() => void pick()}>
			Add from file
		</Button>
	);
}

/** Files dropped on the window install in this game. Is one being dragged over it? */
export function useFileDrop(appId: number, active: boolean, onDrop: () => void): boolean {
	const [dragging, setDragging] = useState(false);
	const dropped = useRef(onDrop);
	dropped.current = onDrop;

	useEffect(() => {
		if (!active || !isTauri()) return;
		let stop: (() => void) | undefined;
		let gone = false;
		void getCurrentWebview()
			.onDragDropEvent(({ payload }) => {
				if (payload.type === "over") return;
				setDragging(payload.type === "enter");
				if (payload.type === "drop") {
					installAll(appId, payload.paths);
					dropped.current();
				}
			})
			.then((unlisten) => {
				if (gone) unlisten();
				else stop = unlisten;
			});
		return () => {
			gone = true;
			stop?.();
			setDragging(false);
		};
	}, [appId, active]);

	return dragging;
}
