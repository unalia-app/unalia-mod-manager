"use client";

import {
	Button,
	Dialog,
	DialogTitle,
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
	Spinner,
	cn,
} from "@modalia/ui";
import { Check, ChevronRight, CircleOff, Copy, Layers, Pencil, Settings2, Trash2, X } from "lucide-react";
import { type FormEvent, type ReactNode, useCallback, useEffect, useState } from "react";

import { toast } from "@/components/toaster";
import {
	type ModProfile,
	createModProfile,
	deleteModProfile,
	modProfiles,
	renameModProfile,
	splitError,
	switchModProfile,
} from "@/lib/engine";
import { useGameStatus } from "@/lib/game-status";
import { refreshInstalled } from "@/lib/installs";

// Not a valid name (they are trimmed): can't collide with a profile.
const NEW = " new";

const NAME_MAX = 40;

// The look of Help & Feedback: crumbs on top, a borderless field, the
// actions in a footer.
const PANEL = "max-w-[520px] gap-0 overflow-hidden rounded-xl p-0";
const FIELD = "w-full bg-transparent text-fg outline-none placeholder:text-white/22";
const CHIP = "flex cursor-pointer items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs font-medium transition-colors";

function fail(error: unknown) {
	const { message, details } = splitError(error);
	toast({ title: message, details: details ?? undefined, tone: "error" });
}

/**
 * Which profile is in use, to switch in one click. A profile is a set of
 * mods turned on: switching turns the others off, nothing is removed.
 */
export function ProfilePicker({
	appId,
	onlyIfSeveral = false,
	className,
}: {
	appId: number;
	/** Nothing while the game has a single profile. */
	onlyIfSeveral?: boolean;
	className?: string;
}) {
	const status = useGameStatus(appId);
	const [profiles, setProfiles] = useState<ModProfile[] | null>(null);
	const [busy, setBusy] = useState(false);
	const [creating, setCreating] = useState(false);
	const [managing, setManaging] = useState(false);

	useEffect(() => {
		modProfiles(appId).then(setProfiles, () => setProfiles(null));
	}, [appId]);

	// The game holds its mods while it runs, and a session without mods has them out.
	const locked = !!status && (status.running || status.starting || status.vanilla);

	const apply = useCallback(
		async (action: () => Promise<ModProfile[]>) => {
			setBusy(true);
			try {
				setProfiles(await action());
			} finally {
				await refreshInstalled(appId).catch(() => {});
				setBusy(false);
			}
		},
		[appId],
	);

	if (!profiles || (onlyIfSeveral && profiles.length < 2)) return null;
	const active = profiles.find((p) => p.active)?.name ?? "";

	const choose = (name: string) => {
		if (name === NEW) setCreating(true);
		else if (name !== active) void apply(() => switchModProfile(appId, name)).catch(fail);
	};

	return (
		<div className={cn("flex shrink-0 items-center gap-1", className)} title={locked ? "Close the game to switch profiles" : undefined}>
			<Select value={active} onValueChange={choose} disabled={busy || locked} className="w-44">
				<SelectTrigger className="h-10">
					<span className="mr-1.5 shrink-0 text-fg-subtle">Profile</span>
					<SelectValue className="min-w-0 truncate" />
				</SelectTrigger>
				<SelectContent>
					{profiles.map((p) => (
						<SelectItem key={p.name} value={p.name} className="truncate">
							{p.name}
						</SelectItem>
					))}
					<SelectItem value={NEW} className="text-fg-muted">
						New profile…
					</SelectItem>
				</SelectContent>
			</Select>
			{profiles.length > 1 && (
				<Button
					size="icon"
					variant="secondary"
					aria-label="Manage profiles"
					title="Rename or delete profiles"
					disabled={busy}
					className="size-10"
					onClick={() => setManaging(true)}
				>
					<Settings2 className="size-4" />
				</Button>
			)}
			{busy && <Spinner className="ml-1 size-4 text-fg-subtle" />}
			{creating && (
				<CreateDialog
					profiles={profiles}
					onCreate={(name, copy) => apply(() => createModProfile(appId, name, copy))}
					onClose={() => setCreating(false)}
				/>
			)}
			{managing && (
				<ManageDialog
					profiles={profiles}
					onRename={(from, to) => apply(() => renameModProfile(appId, from, to))}
					onDelete={(name) => apply(() => deleteModProfile(appId, name))}
					onClose={() => setManaging(false)}
				/>
			)}
		</div>
	);
}

type Start = "empty" | "copy";

const STARTS: { value: Start; label: string; icon: ReactNode }[] = [
	{ value: "empty", label: "Start with no mods", icon: <CircleOff className="size-3.5" /> },
	{ value: "copy", label: "Copy my current mods", icon: <Copy className="size-3.5" /> },
];

/** "Profiles > step": the step is the dialog's title. */
function Crumbs({ step }: { step: string }) {
	return (
		<div className="flex items-center gap-1.5 border-b border-white/5 px-5 py-3">
			<Layers className="size-3.5 text-fg-faint" />
			<span className="text-xs font-medium text-fg-faint">Profiles</span>
			<ChevronRight className="size-3 text-white/20" />
			<DialogTitle className="text-xs font-medium tracking-normal text-fg-subtle">{step}</DialogTitle>
		</div>
	);
}

function Footer({ aside, children }: { aside?: ReactNode; children: ReactNode }) {
	return (
		<div className="flex items-center justify-between gap-3 border-t border-white/5 px-4 py-3">
			<span className="min-w-0 text-xs text-fg-faint">{aside}</span>
			<div className="flex shrink-0 items-center gap-2">{children}</div>
		</div>
	);
}

function CreateDialog({
	profiles,
	onCreate,
	onClose,
}: {
	profiles: ModProfile[];
	onCreate: (name: string, copy: boolean) => Promise<void>;
	onClose: () => void;
}) {
	const [name, setName] = useState("");
	const [start, setStart] = useState<Start>("empty");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const active = profiles.find((p) => p.active)?.name;

	const submit = async (e: FormEvent) => {
		e.preventDefault();
		setBusy(true);
		setError(null);
		try {
			await onCreate(name, start === "copy");
			onClose();
		} catch (err) {
			setError(splitError(err).message);
			setBusy(false);
		}
	};

	return (
		<Dialog open onClose={busy ? undefined : onClose} className={PANEL}>
			<form onSubmit={(e) => void submit(e)}>
				<Crumbs step="New profile" />
				<div className="space-y-4 px-5 py-5">
					<div className="space-y-1.5">
						<input
							autoFocus
							value={name}
							maxLength={NAME_MAX}
							disabled={busy}
							onChange={(e) => {
								setName(e.target.value);
								setError(null);
							}}
							placeholder="Profile name"
							aria-label="Profile name"
							className={cn(FIELD, "text-lg font-medium")}
						/>
						<p className="text-sm text-fg-subtle">
							Its own list of mods. Switching sets the other profiles' mods aside, out of the game: nothing is deleted, and{" "}
							{active ? <span className="text-fg-muted">“{active}”</span> : "this profile"} stays as it is.
						</p>
					</div>
					<div className="flex flex-wrap items-center gap-1.5">
						{STARTS.map((s) => (
							<button
								key={s.value}
								type="button"
								aria-pressed={start === s.value}
								disabled={busy}
								onClick={() => setStart(s.value)}
								className={cn(
									CHIP,
									start === s.value
										? "border-accent/35 bg-accent/12 text-accent"
										: "border-white/10 bg-white/4 text-fg-subtle hover:border-white/18 hover:text-fg-muted",
								)}
							>
								{s.icon}
								{s.label}
							</button>
						))}
					</div>
					{error && <p className="text-xs text-danger">{error}</p>}
				</div>
				<Footer
					aside={
						<span className="font-tight tabular-nums">
							{name.length}/{NAME_MAX}
						</span>
					}
				>
					<Button type="button" variant="secondary" size="sm" disabled={busy} onClick={onClose}>
						Cancel
					</Button>
					<Button type="submit" size="sm" disabled={busy || !name.trim()}>
						{busy ? "Switching…" : "Create and switch"}
					</Button>
				</Footer>
			</form>
		</Dialog>
	);
}

function ManageDialog({
	profiles,
	onRename,
	onDelete,
	onClose,
}: {
	profiles: ModProfile[];
	onRename: (from: string, to: string) => Promise<void>;
	onDelete: (name: string) => Promise<void>;
	onClose: () => void;
}) {
	const [editing, setEditing] = useState<string | null>(null);
	const [draft, setDraft] = useState("");
	const [confirming, setConfirming] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);

	// Mods no other profile has: deleting the profile uninstalls them.
	const only = (p: ModProfile) =>
		p.mods.filter((id) => !profiles.some((other) => other !== p && other.mods.includes(id))).length;

	const run = async (action: Promise<void>) => {
		setError(null);
		try {
			await action;
			setEditing(null);
			setConfirming(null);
		} catch (err) {
			setError(splitError(err).message);
		}
	};

	const rename = (e: FormEvent, from: string) => {
		e.preventDefault();
		void run(draft.trim() === from ? Promise.resolve() : onRename(from, draft));
	};

	return (
		<Dialog open onClose={onClose} className={PANEL}>
			<Crumbs step="Manage" />
			<div className="space-y-2 px-5 py-3">
				<ul>
					{profiles.map((p, i) => (
						<li key={p.name} className={cn("flex h-12 items-center gap-2", i > 0 && "border-t border-white/5")}>
							{editing === p.name ? (
								<form onSubmit={(e) => rename(e, p.name)} className="flex flex-1 items-center gap-1">
									<input
										autoFocus
										value={draft}
										maxLength={NAME_MAX}
										onChange={(e) => setDraft(e.target.value)}
										onKeyDown={(e) => {
											// Escape leaves the field, not the dialog.
											if (e.key !== "Escape") return;
											e.stopPropagation();
											setEditing(null);
										}}
										aria-label={`New name for “${p.name}”`}
										className={cn(FIELD, "text-sm")}
									/>
									<Button type="submit" size="icon" variant="secondary" aria-label="Save the name" disabled={!draft.trim()}>
										<Check className="size-4" />
									</Button>
									<Button type="button" size="icon" variant="secondary" aria-label="Cancel" onClick={() => setEditing(null)}>
										<X className="size-4" />
									</Button>
								</form>
							) : confirming === p.name ? (
								<>
									<span className="min-w-0 flex-1 truncate pb-0.5 text-sm text-fg-muted">
										Uninstall the {only(p)} mod{only(p) === 1 ? "" : "s"} only “{p.name}” has?
									</span>
									<Button size="sm" variant="danger" onClick={() => void run(onDelete(p.name))}>
										Delete
									</Button>
									<Button size="icon" variant="secondary" aria-label="Cancel" onClick={() => setConfirming(null)}>
										<X className="size-4" />
									</Button>
								</>
							) : (
								<>
									<span className="min-w-0 flex-1 truncate pb-0.5 text-sm text-fg">{p.name}</span>
									<span className="shrink-0 font-tight text-xs text-fg-faint">
										{p.active ? "In use" : `${p.mods.length} mod${p.mods.length === 1 ? "" : "s"}`}
									</span>
									<Button
										size="icon"
										variant="secondary"
										aria-label={`Rename “${p.name}”`}
										onClick={() => {
											setEditing(p.name);
											setDraft(p.name);
											setError(null);
										}}
									>
										<Pencil className="size-3.5" />
									</Button>
									<Button
										size="icon"
										variant="secondary"
										aria-label={`Delete “${p.name}”`}
										title={p.active ? "Switch to another profile to delete this one" : undefined}
										disabled={p.active}
										onClick={() => {
											setError(null);
											if (only(p) > 0) setConfirming(p.name);
											else void run(onDelete(p.name));
										}}
									>
										<Trash2 className="size-3.5" />
									</Button>
								</>
							)}
						</li>
					))}
				</ul>
				{error && <p className="text-xs text-danger">{error}</p>}
			</div>
			<Footer aside="A mod stays installed while a profile has it.">
				<Button size="sm" onClick={onClose}>
					Done
				</Button>
			</Footer>
		</Dialog>
	);
}
