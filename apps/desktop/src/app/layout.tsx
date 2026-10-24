import type { Metadata } from "next";
import type { ReactNode } from "react";

import { DevShortcuts } from "@/components/dev-shortcuts";
import { DisplayScale } from "@/components/display-scale";
import { BetaBubble } from "@/components/feedback/beta-bubble";
import { FeedbackDialog } from "@/components/feedback/feedback-dialog";
import { FilePicker } from "@/components/game/file-picker";
import { OverlayScrollbar } from "@/components/overlay-scrollbar";
import { Toaster } from "@/components/toaster";

import { interTight, poppins } from "./fonts";
import "./globals.css";

export const metadata: Metadata = {
	title: "Unalia Mod Manager",
};

export default function RootLayout({ children }: { children: ReactNode }) {
	return (
		<html lang="en" className={`${poppins.variable} ${interTight.variable}`}>
			<body>
				{children}
				<Toaster />
				<OverlayScrollbar />
				<DevShortcuts />
				<DisplayScale />
				<BetaBubble />
				<FeedbackDialog />
				<FilePicker />
			</body>
		</html>
	);
}
