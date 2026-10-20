import type { Metadata } from "next";
import type { ReactNode } from "react";

import { DevShortcuts } from "@/components/dev-shortcuts";
import { ScrollActivity } from "@/components/scroll-activity";
import { Toaster } from "@/components/toaster";

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
				<Toaster />
				<ScrollActivity />
				<DevShortcuts />
			</body>
		</html>
	);
}
