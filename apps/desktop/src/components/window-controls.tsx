"use client";

import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cn } from "@modalia/ui";
import { useEffect, useState } from "react";

/**
 * Boutons de fenêtre maison (Windows, Linux) : la barre de titre native est
 * retirée (`tauri.windows.conf.json`), Modalia dessine les siens. Sous
 * macOS, les feux tricolores du système restent intégrés à l'en-tête.
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
		<div className="-mr-3 flex items-center gap-0.5">
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
				"flex h-8 w-10 cursor-pointer items-center justify-center rounded-md text-fg-muted transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
				// Rouge de fermeture de Windows : le geste attendu, gardé tel quel.
				danger ? "hover:bg-[#c42b1c] hover:text-white" : "hover:bg-surface-strong hover:text-fg",
			)}
		>
			<svg
				viewBox="0 0 12 12"
				className="size-3"
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
