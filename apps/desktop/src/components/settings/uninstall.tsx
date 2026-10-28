"use client";

import { Button, Dialog, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@modalia/ui";
import { Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { toast } from "@/components/toaster";
import { canUninstall, splitError, uninstallApp } from "@/lib/engine";

/**
 * Uninstall without looking for the installer or going through Windows
 * settings: many people remove it after use. Hidden in dev.
 */
export function Uninstall() {
	const [available, setAvailable] = useState(false);
	const [open, setOpen] = useState(false);
	const [starting, setStarting] = useState(false);

	useEffect(() => {
		let current = true;
		canUninstall().then(
			(value) => current && setAvailable(value),
			() => {},
		);
		return () => {
			current = false;
		};
	}, []);

	if (!available) return null;

	const uninstall = async () => {
		setStarting(true);
		try {
			// On success, the app closes: the promise doesn't come back.
			await uninstallApp();
		} catch (error) {
			const { message, details } = splitError(error);
			toast({ title: "Couldn't start the uninstaller", details: details ?? message, tone: "error" });
			setStarting(false);
			setOpen(false);
		}
	};

	return (
		<div className="rounded-xl border border-line bg-white/[0.03] p-5 sm:p-6">
			<h3 className="text-sm font-semibold text-fg">Uninstall</h3>
			<p className="mt-0.5 max-w-xl text-xs leading-relaxed text-fg-subtle">
				Removes Unalia Mod Manager from this PC. You choose in the next step whether your mods leave your games too.
			</p>
			<div className="mt-4">
				<Button variant="outline" leftIcon={<Trash2 className="size-3.5" />} onClick={() => setOpen(true)}>
					Uninstall Unalia
				</Button>
			</div>

			<Dialog open={open} onClose={starting ? undefined : () => setOpen(false)}>
				<DialogHeader>
					<DialogTitle>Uninstall Unalia Mod Manager?</DialogTitle>
					<DialogDescription>
						The app closes and the Windows uninstaller opens. Tick “Delete the application data” to also take every mod
						out of your games and get them back to how they were. Leave it unticked to keep your mods, ready for a
						reinstall.
					</DialogDescription>
				</DialogHeader>
				<DialogFooter>
					<Button variant="secondary" disabled={starting} onClick={() => setOpen(false)}>
						Cancel
					</Button>
					<Button variant="danger" isLoading={starting} onClick={() => void uninstall()}>
						Uninstall
					</Button>
				</DialogFooter>
			</Dialog>
		</div>
	);
}
