import {useEffect, useState} from "react";
import {useQuery, useMutation, useQueryClient} from "@tanstack/react-query";
import {Link} from "@tanstack/react-router";
import {Button, Card} from "@spacedrive/primitives";
import {api} from "@/api/client";
import {LanguageBreakdown} from "./LanguageBreakdown";

// Both upstream's instance-level Project and our codegraph project are
// keyed by string id. We treat them as the same logical entity by reusing
// the upstream project id as the codegraph project id when indexing.

/// Returns codegraph project state for the given project id, or null when
/// no codegraph project exists yet (lets callers show a "start indexing"
/// affordance).
function useCodegraphProject(projectId: string) {
	const queryClient = useQueryClient();

	const query = useQuery({
		queryKey: ["codegraph", "project", projectId],
		queryFn: async () => {
			try {
				return await api.codegraphProject(projectId);
			} catch {
				// 404 — no codegraph project yet for this id.
				return null;
			}
		},
		// Only poll while indexing — SSE drives steady-state updates.
		// Polling at idle leaves orphaned in-flight requests hammering the
		// backend during project switches.
		refetchInterval: (q) => {
			const status = q.state.data?.project.status;
			if (status === "indexing" || status === "pending") return 1_500;
			return false;
		},
	});

	// Cancel in-flight codegraph queries for the outgoing project on
	// unmount / projectId change so a switch doesn't keep CONNECTION_REFUSED-
	// generating fetches alive while the new project's LadybugDB warms up.
	useEffect(() => {
		return () => {
			queryClient.cancelQueries({
				queryKey: ["codegraph", "project", projectId],
			});
		};
	}, [projectId, queryClient]);

	return query;
}

export function LanguageBreakdownSection({projectId}: {projectId: string}) {
	const {data} = useCodegraphProject(projectId);
	const breakdown = data?.project.language_breakdown ?? [];

	if (!data || breakdown.length === 0) return null;

	return (
		<section>
			<h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-ink-faint">
				Languages
			</h3>
			<Card variant="dark" className="p-4">
				<LanguageBreakdown languages={breakdown} />
			</Card>
		</section>
	);
}

export function ReindexSection({
	projectId,
	rootPath,
	projectName,
}: {
	projectId: string;
	rootPath: string;
	projectName: string;
}) {
	const queryClient = useQueryClient();
	const {data, isLoading} = useCodegraphProject(projectId);

	const reindex = useMutation({
		mutationFn: () => api.codegraphReindex(projectId),
		onSuccess: () =>
			queryClient.invalidateQueries({
				queryKey: ["codegraph", "project", projectId],
			}),
	});

	const startIndexing = useMutation({
		mutationFn: () => api.codegraphCreateProject(projectName, rootPath),
		onSuccess: () =>
			queryClient.invalidateQueries({
				queryKey: ["codegraph", "project", projectId],
			}),
	});

	if (isLoading) return null;

	const status = data?.project.status;
	const indexing = status === "pending" || status === "indexing";

	return (
		<section>
			<h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-ink-faint">
				Code Graph
			</h3>
			<Card variant="dark" className="flex items-center justify-between p-4">
				<div>
					<p className="text-sm text-ink">
						{!data
							? "Not indexed"
							: status === "indexed"
								? "Indexed"
								: status === "stale"
									? "Index is stale"
									: status === "error"
										? "Index failed"
										: indexing
											? data.project.progress?.message ?? "Indexing..."
											: "Idle"}
					</p>
				</div>
				{!data ? (
					<Button
						variant="gray"
						size="sm"
						disabled={startIndexing.isPending}
						onClick={() => startIndexing.mutate()}
					>
						{startIndexing.isPending ? "Starting..." : "Start indexing"}
					</Button>
				) : (
					<div className="flex gap-2">
						<Link
							to="/projects/$projectId/codegraph"
							params={{projectId}}
							className="inline-flex items-center rounded-md border border-app-line px-3 py-1.5 text-sm hover:bg-app-hover"
						>
							View graph
						</Link>
						<Button
							variant="gray"
							size="sm"
							disabled={reindex.isPending || indexing}
							onClick={() => reindex.mutate()}
						>
							{indexing ? "Indexing..." : reindex.isPending ? "Queuing..." : "Re-index"}
						</Button>
					</div>
				)}
			</Card>
		</section>
	);
}

/// Derive a human-friendly project name from the trailing folder of a path
/// (e.g. `/Users/alice/code/my-app` -> `my-app`).
export function folderNameFromPath(path: string): string {
	const trimmed = path.trim().replace(/[\\/]+$/, "");
	if (!trimmed) return "";
	const segments = trimmed.split(/[\\/]/);
	return segments[segments.length - 1] ?? "";
}

/// Browse-for-folder dialog. On Tauri it opens a native folder picker via
/// the dialog plugin; on web it falls back to a manual text input.
export function DirectoryBrowserButton({
	onPick,
}: {
	onPick: (path: string) => void;
}) {
	const [busy, setBusy] = useState(false);

	const pick = async () => {
		setBusy(true);
		try {
			// Tauri desktop: use the dialog plugin via the IS_DESKTOP global.
			const desktop = await import("@tauri-apps/plugin-dialog").catch(
				() => null,
			);
			if (desktop && typeof desktop.open === "function") {
				const selected = await desktop.open({
					directory: true,
					multiple: false,
				});
				if (typeof selected === "string" && selected) onPick(selected);
				return;
			}
			// Web: no native picker — caller already exposes a text input.
		} finally {
			setBusy(false);
		}
	};

	return (
		<Button variant="gray" size="sm" disabled={busy} onClick={pick}>
			{busy ? "Opening..." : "Browse..."}
		</Button>
	);
}
