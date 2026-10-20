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
		<html lang="en" className={`${poppins.variable} ${interTight.variable}`}>
			<body>
				{children}
				<DevShortcuts />
			</body>
		</html>
	);
}
