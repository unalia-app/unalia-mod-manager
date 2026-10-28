"use client";

import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";

import { applyInterfaceSize } from "@/lib/display";

/**
 * Applies the chosen interface size at startup (see `lib/display`), then at
 * each screen change: the automatic size depends on the screen.
 */
export function DisplayScale() {
	useEffect(() => {
		void applyInterfaceSize();
		if (!isTauri()) return;
		// Moving the window sends an event per pixel: wait for the end.
		let timer: ReturnType<typeof setTimeout> | undefined;
		const settle = () => {
			clearTimeout(timer);
			timer = setTimeout(() => void applyInterfaceSize(), 300);
		};
		const current = getCurrentWindow();
		const unlisten = Promise.all([current.onMoved(settle), current.onScaleChanged(settle)]);
		return () => {
			clearTimeout(timer);
			void unlisten.then((fns) => {
				for (const fn of fns) fn();
			});
		};
	}, []);
	return null;
}
