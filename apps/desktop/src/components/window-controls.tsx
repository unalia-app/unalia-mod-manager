"use client";

import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cn } from "@modalia/ui";
import { useEffect, useState } from "react";

/**
 * Custom window buttons (Windows, Linux): the native title bar is removed
 * (`tauri.windows.conf.json`), Modalia draws its own. On macOS, the
 * system's traffic lights stay built into the header.
 */
export function WindowControls() {
	const [maximized, setMaximized] = useState(false);

	useEffect(() => {
		if (!isTauri()) return;
		const window = getCurrentWindow();
		void window.isMaximized().then(setMaximized);
		const unlisten = window.onResized(() => void window.isMaximized().then(setMaximized));
		return () => void unlisten.then((stop) => stop());
	}, []);

	if (!isTauri()) return null;
	const window = getCurrentWindow();

	return (
		<div className="flex items-center gap-0.5">
			<ControlButton label="Minimize" onClick={() => void window.minimize()}>
				<path d="M2.5 6h7" />
			</ControlButton>
			<ControlButton label={maximized ? "Restore" : "Maximize"} onClick={() => void window.toggleMaximize()}>
				{maximized ? (
					<>
						<rect x="2.5" y="4" width="5.5" height="5.5" rx="1.2" />
						<path d="M4.5 4V3.2c0-.66.54-1.2 1.2-1.2h3.1c.66 0 1.2.54 1.2 1.2v3.1c0 .66-.54 1.2-1.2 1.2H8" />
					</>
				) : (
					<rect x="2.5" y="2.5" width="7" height="7" rx="1.4" />
				)}
			</ControlButton>
			<ControlButton label="Close" danger onClick={() => void window.close()}>
				<path d="M3 3l6 6M9 3l-6 6" />
			</ControlButton>
		</div>
	);
}

function ControlButton({
	label,
	danger = false,
	onClick,
	children,
}: {
	label: string;
	danger?: boolean;
	onClick: () => void;
	children: React.ReactNode;
}) {
	return (
		<button
			type="button"
			aria-label={label}
			title={label}
			onClick={onClick}
			className={cn(
				"flex h-9 w-11 cursor-pointer items-center justify-center rounded-md text-fg-muted transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
				// Windows close red: the expected cue, kept as is.
				danger ? "hover:bg-[#c42b1c] hover:text-white" : "hover:bg-surface-strong hover:text-fg",
			)}
		>
			<svg
				viewBox="0 0 12 12"
				className="size-3.5"
				fill="none"
				stroke="currentColor"
				strokeWidth="1.2"
				strokeLinecap="round"
				strokeLinejoin="round"
				aria-hidden
			>
				{children}
			</svg>
		</button>
	);
}
