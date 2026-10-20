import type { Metadata } from "next";
import type { ReactNode } from "react";

import { DevShortcuts } from "@/components/dev-shortcuts";

import { interTight, poppins } from "./fonts";
import "./globals.css";

export const metadata: Metadata = {
	title: "Modalia",
};

export default function RootLayout({ children }: { children: ReactNode }) {
	return (
		<html lang="fr" className={`${poppins.variable} ${interTight.variable}`}>
			<body>
				{/* Barre de titre : la fenêtre n'a pas de barre système (macOS),
				    cette zone sert à la déplacer. */}
				<div data-tauri-drag-region className="fixed inset-x-0 top-0 z-50 h-11" />
				{children}
				<DevShortcuts />
			</body>
		</html>
	);
}
