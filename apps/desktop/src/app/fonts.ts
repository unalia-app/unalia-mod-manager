import { Inter_Tight, Poppins } from "next/font/google";

// Same fonts as Unalia. next/font downloads them at build time and serves
// them from the app: no network call at runtime.
export const poppins = Poppins({
	subsets: ["latin"],
	weight: ["300", "400", "500", "600", "700"],
	variable: "--font-poppins",
});

export const interTight = Inter_Tight({
	subsets: ["latin"],
	variable: "--font-inter-tight",
});
