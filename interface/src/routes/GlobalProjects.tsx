import { useState, useEffect, useCallback } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { api, type CodeGraphProject, type CodeGraphIndexStatus, type DirEntry, type CodeGraphProjectListResponse, type CodeGraphProjectDetailResponse } from "@/api/client";
import { Badge, Button } from "@/ui";
import { LanguageBreakdown } from "@/components/projects/LanguageBreakdown";
import {
	Dialog,
	DialogContent,
	DialogHeader,
	DialogTitle,
	DialogFooter,
	DialogDescription,
} from "@/ui/Dialog";
import { Input, Label } from "@/ui/Input";
import { clsx } from "clsx";
import { AnimatePresence, motion } from "framer-motion";
import { useServer } from "@/hooks/useServer";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const STATUS_CONFIG: Record<
	CodeGraphIndexStatus,
	{ label: string; color: string; dot: string }
> = {
	indexed: { label: "Indexed", color: "text-emerald-400", dot: "bg-emerald-500" },
	indexing: { label: "Indexing", color: "text-blue-400", dot: "bg-blue-500" },
	stale: { label: "Stale", color: "text-amber-400", dot: "bg-amber-500" },
	error: { label: "Error", color: "text-red-400", dot: "bg-red-500" },
	pending: { label: "Pending", color: "text-amber-400", dot: "bg-amber-500" },
};

function StatusBadge({ status, progress }: { status: CodeGraphIndexStatus; progress?: CodeGraphProject["progress"] }) {
	const cfg = STATUS_CONFIG[status] ?? STATUS_CONFIG.pending;
	const label = status === "indexing" && progress
		? `Indexing ${progress.phase}`
		: cfg.label;

	return (
		<span className={clsx("inline-flex items-center gap-1.5 text-xs font-medium", cfg.color)}>
			<span className={clsx("h-1.5 w-1.5 rounded-full", cfg.dot)} />
			{label}
		</span>
	);
}

function formatNumber(n: number): string {
	if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
	return String(n);
}

// ---------------------------------------------------------------------------
// Project Card
// ---------------------------------------------------------------------------

const PIPELINE_PHASES = [
	"extracting", "structure", "parsing", "imports", "calls",
	"heritage", "communities", "processes", "enriching", "complete",
] as const;

const PHASE_COLORS: Record<string, string> = {
	extracting: "bg-emerald-500", structure: "bg-emerald-400", parsing: "bg-teal-500",
	imports: "bg-blue-500", calls: "bg-blue-400", heritage: "bg-indigo-500",
	communities: "bg-violet-500", processes: "bg-purple-500",
	enriching: "bg-amber-500", complete: "bg-emerald-500",
};

function ProjectCard({ project }: { project: CodeGraphProject }) {
	const queryClient = useQueryClient();
	const [removeOpen, setRemoveOpen] = useState(false);
	const reindexMutation = useMutation({
		mutationFn: () => api.codegraphReindex(project.project_id),
		onMutate: async () => {
			const listKey = ["codegraph-projects"];
			const detailKey = ["codegraph-project", project.project_id];
			await Promise.all([
				queryClient.cancelQueries({ queryKey: listKey }),
				queryClient.cancelQueries({ queryKey: detailKey }),
			]);
			const prevList = queryClient.getQueryData<CodeGraphProjectListResponse>(listKey);
			const prevDetail = queryClient.getQueryData<CodeGraphProjectDetailResponse>(detailKey);
			if (prevList) {
				queryClient.setQueryData<CodeGraphProjectListResponse>(listKey, {
					projects: prevList.projects.map((p) =>
						p.project_id === project.project_id ? { ...p, status: "indexing" } : p,
					),
				});
			}
			if (prevDetail) {
				queryClient.setQueryData<CodeGraphProjectDetailResponse>(detailKey, {
					project: { ...prevDetail.project, status: "indexing" },
				});
			}
			return { prevList, prevDetail };
		},
		onError: (_err, _vars, ctx) => {
			if (ctx?.prevList) queryClient.setQueryData(["codegraph-projects"], ctx.prevList);
			if (ctx?.prevDetail) queryClient.setQueryData(["codegraph-project", project.project_id], ctx.prevDetail);
		},
		onSettled: () => {
			queryClient.invalidateQueries({ queryKey: ["codegraph-projects"] });
			queryClient.invalidateQueries({ queryKey: ["codegraph-project", project.project_id] });
		},
	});

	const isIndexing = project.status === "indexing";
	const needsReindex = project.status === "pending" && !!project.last_indexed_at;
	const progress = project.progress;
	const stats = isIndexing ? progress?.stats : project.last_index_stats;

	// Compute overall progress percentage from phase position.
	const phaseIdx = progress
		? PIPELINE_PHASES.indexOf(progress.phase as typeof PIPELINE_PHASES[number])
		: -1;
	const overallPct = isIndexing && phaseIdx >= 0
		? Math.round(((phaseIdx + (progress?.phase_progress ?? 0)) / PIPELINE_PHASES.length) * 100)
		: 0;
	return (
		<motion.div
			layout
			initial={{ opacity: 0, y: 8 }}
			animate={{ opacity: 1, y: 0 }}
			exit={{ opacity: 0, y: -8 }}
			className="rounded-xl border border-app-line bg-app-darkBox p-5 transition-colors hover:border-accent/30"
		>
			<div className="flex items-start justify-between gap-3">
				<div className="flex min-w-0 items-center gap-3">
					<span className="flex h-8 w-8 items-center justify-center rounded-lg bg-accent/10 text-sm text-accent">
						{project.name.charAt(0).toUpperCase()}
					</span>
					<div className="min-w-0">
						<h3 className="truncate font-plex text-sm font-semibold text-ink">
							{project.name}
						</h3>
						<p className="truncate text-xs text-ink-faint">{project.root_path}</p>
					</div>
				</div>
				<div className="flex items-center gap-2">
					<StatusBadge status={project.status} progress={project.progress} />
					<button
						type="button"
						onClick={(e) => { e.preventDefault(); e.stopPropagation(); setRemoveOpen(true); }}
						title="Remove project"
						aria-label="Remove project"
						className="rounded p-1 text-ink-faint transition-colors hover:bg-red-500/10 hover:text-red-400"
					>
						<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
							<polyline points="3 6 5 6 21 6" />
							<path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
							<path d="M10 11v6" />
							<path d="M14 11v6" />
							<path d="M9 6V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2" />
						</svg>
					</button>
				</div>
			</div>

			{/* Schema upgrade banner */}
			{needsReindex && (
				<div className="mt-3 flex items-center gap-2 rounded-lg border border-amber-500/20 bg-amber-500/5 px-3 py-2">
					<span className="flex-1 text-xs text-amber-400">Re-index required — engine updated</span>
					<Button
						size="sm"
						onClick={(e) => { e.preventDefault(); reindexMutation.mutate(); }}
						disabled={reindexMutation.isPending}
						className="h-6 px-2 text-[10px]"
					>
						{reindexMutation.isPending ? "Starting..." : "Re-index"}
					</Button>
				</div>
			)}

			{/* Indexing progress bar — segmented by phase */}
			{isIndexing && progress && (
				<div className="mt-3">
					<div className="mb-1.5 flex items-center justify-between text-[10px]">
						<span className="text-ink-dull">{progress.message}</span>
						<span className="text-ink-faint">{overallPct}%</span>
					</div>
					<div className="flex h-1.5 gap-px overflow-hidden rounded-full">
						{PIPELINE_PHASES.map((phase, i) => {
							const isDone = i < phaseIdx;
							const isCurrent = i === phaseIdx;
							const fillPct = isDone ? 100 : isCurrent ? Math.round((progress.phase_progress ?? 0) * 100) : 0;
							return (
								<div key={phase} className="relative flex-1 overflow-hidden rounded-full bg-app-line">
									<div
										className={clsx(
											"absolute inset-y-0 left-0 rounded-full transition-all duration-700 ease-out",
											PHASE_COLORS[phase] ?? "bg-accent",
										)}
										style={{ width: `${fillPct}%` }}
									/>
								</div>
							);
						})}
					</div>
				</div>
			)}

			{/* Stats */}
			{stats && (
				<div className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-xs text-ink-dull">
					<span>{formatNumber(stats.nodes_created)} nodes</span>
					<span>{formatNumber(stats.edges_created)} edges</span>
					<span>{formatNumber(stats.communities_detected)} communities</span>
					<span>{formatNumber(stats.files_found ?? stats.files_parsed)} files</span>
				</div>
			)}

			{project.status === "error" && project.error_message && (
				<p className="mt-2 truncate text-xs text-red-400/80" title={project.error_message}>
					{project.error_message}
				</p>
			)}

			{project.language_breakdown && project.language_breakdown.length > 0 ? (
				<div className="mt-3">
					<LanguageBreakdown breakdown={project.language_breakdown} />
				</div>
			) : project.primary_language ? (
				<div className="mt-2">
					<Badge variant="default" size="sm">{project.primary_language}</Badge>
				</div>
			) : null}

			{/* Actions */}
			<div className="mt-4 flex items-center gap-2">
				<Link
					to="/projects/$projectId"
					params={{ projectId: project.project_id }}
					className="text-xs font-medium text-accent hover:underline"
				>
					View Details
				</Link>
			</div>

			<RemoveProjectDialog
				project={project}
				open={removeOpen}
				onOpenChange={setRemoveOpen}
			/>
		</motion.div>
	);
}

// ---------------------------------------------------------------------------
// Remove Project Dialog
// ---------------------------------------------------------------------------

function RemoveProjectDialog({
	project,
	open,
	onOpenChange,
}: {
	project: CodeGraphProject;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}) {
	const queryClient = useQueryClient();

	const { data: removeInfo } = useQuery({
		queryKey: ["codegraph-remove-info", project.project_id],
		queryFn: () => api.codegraphRemoveInfo(project.project_id),
		enabled: open,
	});

	const mutation = useMutation({
		mutationFn: () => api.codegraphDeleteProject(project.project_id),
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: ["codegraph-projects"] });
			onOpenChange(false);
		},
	});

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent>
				<DialogHeader>
					<DialogTitle>Remove Project: {project.name}?</DialogTitle>
					<DialogDescription>This will permanently delete:</DialogDescription>
				</DialogHeader>
				<div className="flex flex-col gap-2 py-4 text-sm text-ink-dull">
					{removeInfo && (
						<>
							<p>Code graph index ({removeInfo.node_count.toLocaleString()} nodes, {removeInfo.edge_count.toLocaleString()} edges)</p>
							<p>All index history and logs</p>
						</>
					)}
					<p className="mt-2 font-medium text-red-400">This cannot be undone.</p>
				</div>
				<DialogFooter>
					<Button variant="ghost" onClick={() => onOpenChange(false)}>
						Cancel
					</Button>
					<Button
						variant="destructive"
						onClick={() => mutation.mutate()}
						disabled={mutation.isPending}
					>
						{mutation.isPending ? "Removing..." : "Remove Project"}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}

// ---------------------------------------------------------------------------
// Native OS folder dialog (Tauri only)
// ---------------------------------------------------------------------------

async function openNativeFolderDialog(): Promise<string | null> {
	try {
		const { open } = await import("@tauri-apps/plugin-dialog");
		const selected = await open({
			directory: true,
			multiple: false,
			title: "Select Project Directory",
		});
		return typeof selected === "string" ? selected : null;
	} catch {
		return null;
	}
}

// ---------------------------------------------------------------------------
// Web fallback directory browser
// ---------------------------------------------------------------------------

function DirectoryBrowser({
	onSelect,
	onClose,
}: {
	onSelect: (path: string) => void;
	onClose: () => void;
}) {
	const [currentPath, setCurrentPath] = useState<string>("");
	const [entries, setEntries] = useState<DirEntry[]>([]);
	const [parentPath, setParentPath] = useState<string | null>(null);
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);

	const loadDir = useCallback(async (path?: string) => {
		setLoading(true);
		setError(null);
		try {
			const result = await api.listDir(path);
			setCurrentPath(result.path);
			setParentPath(result.parent);
			setEntries(result.entries.filter((e) => e.is_dir));
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load directory");
		} finally {
			setLoading(false);
		}
	}, []);

	useEffect(() => {
		loadDir();
	}, [loadDir]);

	return (
		<div className="rounded-lg border border-app-line bg-app-darkBox">
			<div className="flex items-center gap-2 border-b border-app-line px-3 py-2">
				<button
					type="button"
					onClick={() => parentPath && loadDir(parentPath)}
					disabled={!parentPath}
					className="rounded px-1.5 py-0.5 text-xs text-ink-dull hover:bg-app-hover/40 disabled:opacity-30"
				>
					..
				</button>
				<span className="min-w-0 flex-1 truncate font-mono text-xs text-ink-dull">
					{currentPath}
				</span>
				<Button type="button" size="sm" onClick={() => onSelect(currentPath)}>
					Select
				</Button>
				<button
					type="button"
					onClick={onClose}
					className="rounded px-1.5 py-0.5 text-xs text-ink-faint hover:text-ink"
				>
					&times;
				</button>
			</div>
			<div className="max-h-48 overflow-y-auto">
				{loading && (
					<div className="px-3 py-4 text-center text-xs text-ink-faint">Loading...</div>
				)}
				{error && (
					<div className="px-3 py-4 text-center text-xs text-red-400">{error}</div>
				)}
				{!loading && !error && entries.length === 0 && (
					<div className="px-3 py-4 text-center text-xs text-ink-faint">No subdirectories</div>
				)}
				{!loading &&
					!error &&
					entries.map((entry) => (
						<button
							key={entry.path}
							type="button"
							onClick={() => loadDir(entry.path)}
							className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm text-ink hover:bg-app-hover/40"
						>
							<span className="text-xs text-accent">&#128193;</span>
							<span className="truncate">{entry.name}</span>
						</button>
					))}
			</div>
		</div>
	);
}

// ---------------------------------------------------------------------------
// Create Project Dialog
// ---------------------------------------------------------------------------

function CreateProjectDialog({
	open,
	onOpenChange,
}: {
	open: boolean;
	onOpenChange: (open: boolean) => void;
}) {
	const queryClient = useQueryClient();
	const { isTauri } = useServer();
	const [name, setName] = useState("");
	const [nameManuallyEdited, setNameManuallyEdited] = useState(false);
	const [rootPath, setRootPath] = useState("");
	const [showBrowser, setShowBrowser] = useState(false);

	// Auto-fill the project name from the folder name when a path is
	// selected, unless the user has manually typed a custom name.
	const updateRootPath = (path: string) => {
		setRootPath(path);
		if (!nameManuallyEdited) {
			const segments = path.replace(/[\\/]+$/, "").split(/[\\/]/);
			const folderName = segments[segments.length - 1] || "";
			setName(folderName);
		}
	};

	const mutation = useMutation({
		mutationFn: () => api.codegraphCreateProject(name, rootPath),
		onError: (err) => {
			console.error("Failed to create codegraph project:", err);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: ["codegraph-projects"] });
			onOpenChange(false);
			setName("");
			setNameManuallyEdited(false);
			setRootPath("");
			setShowBrowser(false);
		},
	});

	const handleBrowse = async () => {
		if (isTauri) {
			const selected = await openNativeFolderDialog();
			if (selected) updateRootPath(selected);
		} else {
			setShowBrowser((prev) => !prev);
		}
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent>
				<DialogHeader>
					<DialogTitle>Add Project</DialogTitle>
					<DialogDescription>
						Add a project to index its code graph. Indexing starts automatically.
					</DialogDescription>
				</DialogHeader>
				<div className="flex flex-col gap-4 py-4">
					<div>
						<Label htmlFor="project-name">Project Name</Label>
						<Input
							id="project-name"
							value={name}
							onChange={(e) => {
								setName(e.target.value);
								setNameManuallyEdited(true);
							}}
							placeholder="my-project"
						/>
					</div>
					<div>
						<Label htmlFor="root-path">Root Path</Label>
						<div className="flex gap-2">
							<Input
								id="root-path"
								value={rootPath}
								onChange={(e) => updateRootPath(e.target.value)}
								placeholder="/path/to/project"
								className="flex-1 font-mono"
							/>
							<Button
								type="button"
								variant="outline"
								onClick={handleBrowse}
								title="Browse for directory"
							>
								Browse
							</Button>
						</div>
						{showBrowser && !isTauri && (
							<div className="mt-2">
								<DirectoryBrowser
									onSelect={(path) => {
										updateRootPath(path);
										setShowBrowser(false);
									}}
									onClose={() => setShowBrowser(false)}
								/>
							</div>
						)}
					</div>
				</div>
				{mutation.isError && (
					<p className="text-sm text-red-400">
						Error: {mutation.error instanceof Error ? mutation.error.message : "Failed to create project"}
					</p>
				)}
				<DialogFooter>
					<Button
						variant="ghost"
						onClick={() => onOpenChange(false)}
					>
						Cancel
					</Button>
					<Button
						onClick={() => mutation.mutate()}
						disabled={!name || !rootPath || mutation.isPending}
					>
						{mutation.isPending ? "Adding..." : "Add Project"}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}

// ---------------------------------------------------------------------------
// Main Component
// ---------------------------------------------------------------------------

export function GlobalProjects() {
	const queryClient = useQueryClient();
	const [createOpen, setCreateOpen] = useState(false);
	const [statusFilter, setStatusFilter] = useState<CodeGraphIndexStatus | "all">("all");

	const { data, isLoading } = useQuery({
		queryKey: ["codegraph-projects"],
		queryFn: () => api.codegraphProjects(),
		refetchInterval: (query) => {
			const hasIndexing = query.state.data?.projects?.some((p) => p.status === "indexing");
			return hasIndexing ? 2_000 : 10_000;
		},
	});

	// Live-refresh the list when the file watcher reports a graph change or
	// completed re-index. Without this the language breakdown on each card
	// only updates on the 10s idle poll — a file add/delete would lag.
	useEffect(() => {
		const source = new EventSource(api.getEventsUrl());
		const handle = (e: MessageEvent) => {
			try {
				const event = JSON.parse(e.data);
				if (event.type === "code_graph_changed" || event.type === "code_graph_indexed") {
					queryClient.invalidateQueries({ queryKey: ["codegraph-projects"] });
					if (event.project_id) {
						queryClient.invalidateQueries({ queryKey: ["codegraph-project", event.project_id] });
					}
				}
			} catch { /* ignore parse errors */ }
		};
		source.addEventListener("message", handle);
		return () => source.close();
	}, [queryClient]);

	const projects = data?.projects ?? [];
	const needsReindex = projects.filter((p) => p.status === "pending" && p.last_indexed_at);
	const filtered = statusFilter === "all"
		? projects
		: projects.filter((p) => p.status === statusFilter);

	const reindexAllMutation = useMutation({
		mutationFn: async () => {
			await Promise.all(needsReindex.map((p) => api.codegraphReindex(p.project_id)));
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: ["codegraph-projects"] });
		},
	});

	return (
		<div className="flex h-full flex-col overflow-y-auto p-6">
			{/* Header */}
			<div className="mb-6 flex items-center justify-between">
				<div>
					<h2 className="font-plex text-lg font-semibold text-ink">Projects</h2>
					<p className="text-sm text-ink-faint">
						{projects.length} project{projects.length !== 1 ? "s" : ""} indexed
					</p>
				</div>
				<Button onClick={() => setCreateOpen(true)}>+ Add Project</Button>
			</div>

			{/* Schema upgrade banner */}
			{needsReindex.length > 0 && (
				<div className="mb-4 flex items-center gap-3 rounded-xl border border-amber-500/20 bg-amber-500/5 px-4 py-3">
					<div className="h-2 w-2 rounded-full bg-amber-500" />
					<p className="flex-1 text-sm text-amber-400">
						{needsReindex.length} project{needsReindex.length !== 1 ? "s need" : " needs"} re-indexing after engine update
					</p>
					<Button
						size="sm"
						onClick={() => reindexAllMutation.mutate()}
						disabled={reindexAllMutation.isPending}
					>
						{reindexAllMutation.isPending ? "Starting..." : `Re-index All (${needsReindex.length})`}
					</Button>
				</div>
			)}

			{/* Filter */}
			<div className="mb-4 flex gap-2">
				{(["all", "indexed", "indexing", "pending", "stale", "error"] as const).map((s) => (
					<button
						key={s}
						onClick={() => setStatusFilter(s)}
						className={clsx(
							"rounded-md px-3 py-1 text-xs font-medium transition-colors",
							statusFilter === s
								? "bg-accent/20 text-accent"
								: "text-ink-dull hover:bg-app-selected/50",
						)}
					>
						{s === "all" ? "All" : s.charAt(0).toUpperCase() + s.slice(1)}
					</button>
				))}
			</div>

			{/* Grid */}
			{isLoading ? (
				<div className="flex flex-1 items-center justify-center">
					<p className="text-sm text-ink-faint">Loading projects...</p>
				</div>
			) : filtered.length === 0 ? (
				<div className="flex flex-1 flex-col items-center justify-center gap-3">
					<p className="text-sm text-ink-faint">
						{projects.length === 0
							? "No projects indexed yet"
							: "No projects match this filter"}
					</p>
					{projects.length === 0 && (
						<Button onClick={() => setCreateOpen(true)} variant="ghost">
							Add your first project
						</Button>
					)}
				</div>
			) : (
				<AnimatePresence mode="popLayout">
					<div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
						{filtered.map((project) => (
							<ProjectCard key={project.project_id} project={project} />
						))}
					</div>
				</AnimatePresence>
			)}

			<CreateProjectDialog open={createOpen} onOpenChange={setCreateOpen} />
		</div>
	);
}
