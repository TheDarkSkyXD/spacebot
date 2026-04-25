// Right-side panel for the mermaid view. Renders the project overview by
// default and swaps to the selected-file info card when a node is chosen.
// Drag-resizable width persisted to localStorage; mirrors the
// CodeInspectorPanel ergonomics.

import { useCallback, useEffect, useRef, useState } from "react";
import type { CodeGraphBulkEdgeSummary } from "@/api/client";
import type { BulkNode } from "./types";
import type { BuildResult } from "./mermaidGraphBuilder";
import { ProjectOverviewCard } from "./ProjectOverviewCard";
import { NodeInfoCard } from "./NodeInfoCard";

interface Props {
	projectName: string;
	graph: BuildResult;
	allNodes: BulkNode[];
	allEdges: CodeGraphBulkEdgeSummary[];
	selectedNode: BulkNode | null;
	onSelectFile: (file: BulkNode | null) => void;
	onRequestSource: () => void;
}

const MIN_WIDTH = 300;
const MAX_WIDTH = 640;
const DEFAULT_WIDTH = 380;
const STORAGE_KEY = "spacebot.codegraph.mermaidPanelWidth";

export function MermaidRightPanel({ projectName, graph, allNodes, allEdges, selectedNode, onSelectFile, onRequestSource }: Props) {
	const [width, setWidth] = useState<number>(() => {
		try {
			const saved = window.localStorage.getItem(STORAGE_KEY);
			const parsed = saved ? parseInt(saved, 10) : NaN;
			if (!Number.isFinite(parsed)) return DEFAULT_WIDTH;
			return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, parsed));
		} catch {
			return DEFAULT_WIDTH;
		}
	});

	const dragState = useRef<{ startX: number; startWidth: number } | null>(null);

	useEffect(() => {
		try { window.localStorage.setItem(STORAGE_KEY, String(width)); } catch { /* ignore */ }
	}, [width]);

	const onMouseDown = useCallback((e: React.MouseEvent) => {
		if (e.button !== 0) return;
		dragState.current = { startX: e.clientX, startWidth: width };
		e.preventDefault();
	}, [width]);

	useEffect(() => {
		const onMove = (e: MouseEvent) => {
			const d = dragState.current;
			if (!d) return;
			const delta = d.startX - e.clientX;
			const next = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, d.startWidth + delta));
			setWidth(next);
		};
		const onUp = () => { dragState.current = null; };
		window.addEventListener("mousemove", onMove);
		window.addEventListener("mouseup", onUp);
		return () => {
			window.removeEventListener("mousemove", onMove);
			window.removeEventListener("mouseup", onUp);
		};
	}, []);

	return (
		<div
			className="relative flex h-full shrink-0 flex-col border-l border-app-line"
			style={{ width }}
		>
			<span
				onMouseDown={onMouseDown}
				className="absolute inset-y-0 left-0 z-10 w-1 -translate-x-0.5 cursor-col-resize bg-transparent hover:bg-accent/30"
			/>
			{selectedNode ? (
				<NodeInfoCard
					selected={selectedNode}
					graph={graph}
					allNodes={allNodes}
					allEdges={allEdges}
					onSelectFile={onSelectFile}
					onRequestSource={onRequestSource}
				/>
			) : (
				<ProjectOverviewCard
					projectName={projectName}
					graph={graph}
					allNodes={allNodes}
					edgeCount={allEdges.length}
					onSelectFile={(file) => onSelectFile(file)}
				/>
			)}
		</div>
	);
}
