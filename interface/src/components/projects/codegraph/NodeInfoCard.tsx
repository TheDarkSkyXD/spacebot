// Right-panel content shown in the mermaid view when a file node is
// selected. File name + path, list of symbols defined in the file, and
// a typed list of connections (imports / calls / extends / implements,
// both directions) that can be clicked to navigate selection.

import { useMemo } from "react";
import type { CodeGraphBulkEdgeSummary } from "@/api/client";
import { NODE_COLORS, type NodeLabel } from "./constants";
import type { BuildResult } from "./mermaidGraphBuilder";
import type { BulkNode } from "./types";

interface Props {
	selected: BulkNode;
	graph: BuildResult;
	allNodes: BulkNode[];
	allEdges: CodeGraphBulkEdgeSummary[];
	onSelectFile: (file: BulkNode | null) => void;
	onRequestSource: () => void;
}

const RELATION_TYPES: readonly string[] = ["IMPORTS", "CALLS", "EXTENDS", "IMPLEMENTS"] as const;

const EDGE_LABELS_OUT: Record<string, string> = {
	IMPORTS: "→ imports",
	CALLS: "→ calls",
	EXTENDS: "→ extends",
	IMPLEMENTS: "→ implements",
};
const EDGE_LABELS_IN: Record<string, string> = {
	IMPORTS: "← imported by",
	CALLS: "← called by",
	EXTENDS: "← extended by",
	IMPLEMENTS: "← implemented by",
};

function formatBytes(bytes: number | null | undefined): string | null {
	if (bytes == null) return null;
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface Connection {
	direction: "out" | "in";
	type: string;
	other: BulkNode;
}

export function NodeInfoCard({ selected, graph, allNodes, allEdges, onSelectFile, onRequestSource }: Props) {
	const selectedIsFile = selected.label === "File";

	// Resolve the file behind the selected node. If a symbol was passed in,
	// find its parent file.
	const focusFile = useMemo<BulkNode | null>(() => {
		if (selectedIsFile) return selected;
		if (!selected.source_file) return null;
		return graph.fileNodes.find((f) => f.source_file === selected.source_file) ?? null;
	}, [selected, selectedIsFile, graph.fileNodes]);

	const fileQname = focusFile?.qualified_name ?? null;
	const size = formatBytes(focusFile?.file_size);
	const symbols = focusFile ? graph.symbolsByFile.get(focusFile.qualified_name) ?? [] : [];

	// Compute connections: walk the full edge set once, matching the focused
	// file against either endpoint via the qname → fileQname map the graph
	// builder would have produced. We rebuild the map inline to avoid
	// coupling the builder to this component.
	const connections = useMemo<Connection[]>(() => {
		if (!fileQname) return [];
		const fileByPath = new Map<string, BulkNode>();
		for (const f of graph.fileNodes) {
			if (f.source_file) fileByPath.set(f.source_file, f);
		}
		const qnameToFile = new Map<string, BulkNode>();
		for (const node of allNodes) {
			if (!node.source_file) continue;
			const parent = fileByPath.get(node.source_file);
			if (parent) qnameToFile.set(node.qualified_name, parent);
		}
		for (const f of graph.fileNodes) qnameToFile.set(f.qualified_name, f);

		const seen = new Set<string>();
		const out: Connection[] = [];
		for (const edge of allEdges) {
			if (!RELATION_TYPES.includes(edge.edge_type)) continue;
			const fromFile = qnameToFile.get(edge.from_qname);
			const toFile = qnameToFile.get(edge.to_qname);
			if (!fromFile || !toFile || fromFile.qualified_name === toFile.qualified_name) continue;
			if (fromFile.qualified_name === fileQname) {
				const key = `out\u0001${edge.edge_type}\u0001${toFile.qualified_name}`;
				if (seen.has(key)) continue;
				seen.add(key);
				out.push({ direction: "out", type: edge.edge_type, other: toFile });
			} else if (toFile.qualified_name === fileQname) {
				const key = `in\u0001${edge.edge_type}\u0001${fromFile.qualified_name}`;
				if (seen.has(key)) continue;
				seen.add(key);
				out.push({ direction: "in", type: edge.edge_type, other: fromFile });
			}
		}
		return out;
	}, [fileQname, graph.fileNodes, allNodes, allEdges]);

	if (!focusFile) {
		return (
			<div className="flex h-full flex-col overflow-hidden bg-app-darkBox text-ink">
				<div className="flex items-center gap-2 border-b border-app-line px-5 py-3">
					<button
						type="button"
						onClick={() => onSelectFile(null)}
						className="text-[11px] text-ink-dull hover:text-ink"
					>
						← Back
					</button>
				</div>
				<div className="flex-1 px-5 py-4">
					<p className="text-sm text-ink-dull">Selection has no file context.</p>
				</div>
			</div>
		);
	}

	const displayName = focusFile.name;

	return (
		<div className="flex h-full flex-col overflow-hidden bg-app-darkBox text-ink">
			<div className="flex items-center gap-2 border-b border-app-line px-5 py-3">
				<button
					type="button"
					onClick={() => onSelectFile(null)}
					className="text-[11px] text-ink-dull hover:text-ink"
				>
					← Back to overview
				</button>
			</div>

			<div className="flex-1 overflow-y-auto px-5 py-4">
				{size && (
					<div className="flex items-center gap-2">
						<span className="rounded border border-app-line bg-app px-2 py-0.5 font-mono text-[10px] text-ink-faint">
							{size}
						</span>
					</div>
				)}
				<h2 className="mt-2 break-all text-base font-semibold text-ink">{displayName}</h2>
				{focusFile.source_file && (
					<p className="mt-0.5 break-all font-mono text-[11px] text-ink-dull">
						{focusFile.source_file}
					</p>
				)}

				<button
					type="button"
					onClick={onRequestSource}
					className="mt-3 rounded-md border border-accent/40 bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent transition-colors hover:bg-accent/20"
				>
					View source
				</button>

				{symbols.length > 0 && (
					<section className="mt-6">
						<h3 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-ink-faint">
							Defined in this file
						</h3>
						<ul className="space-y-1">
							{symbols.map((sym) => (
								<li
									key={sym.qualified_name}
									className="flex items-center gap-2 rounded-md border border-app-line bg-app px-3 py-1.5 text-[11px] text-ink-dull"
								>
									<span
										className="h-1.5 w-1.5 shrink-0 rounded-full"
										style={{ background: NODE_COLORS[sym.label as NodeLabel] ?? "#64748b" }}
									/>
									<span className="font-mono text-[10px] text-ink-faint">{sym.label}</span>
									<span className="truncate text-ink">{sym.name}</span>
								</li>
							))}
						</ul>
					</section>
				)}

				{connections.length > 0 && (
					<section className="mt-6">
						<h3 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-ink-faint">
							Connections ({connections.length})
						</h3>
						<ul className="space-y-1">
							{connections.map((c) => {
								const label = c.direction === "out"
									? EDGE_LABELS_OUT[c.type] ?? c.type
									: EDGE_LABELS_IN[c.type] ?? c.type;
								return (
									<li key={`${c.direction}\u0001${c.type}\u0001${c.other.qualified_name}`}>
										<button
											type="button"
											onClick={() => onSelectFile(c.other)}
											className="flex w-full items-center gap-2 rounded-md border border-app-line bg-app px-3 py-1.5 text-left text-[11px] transition-colors hover:border-accent hover:bg-app-hover"
										>
											<span className="font-mono text-[10px] text-ink-faint">{label}</span>
											<span className="truncate text-ink">{c.other.name}</span>
										</button>
									</li>
								);
							})}
						</ul>
					</section>
				)}

				{connections.length === 0 && symbols.length === 0 && (
					<p className="mt-6 text-xs text-ink-faint">
						No indexed symbols or relationships for this file.
					</p>
				)}
			</div>
		</div>
	);
}
