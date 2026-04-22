// Mermaid-mode graph view — renders the code graph as files grouped by
// relationship clusters (union-find over cross-file edges). Each group is
// a React Flow parent container with an editable title, color swatch, and
// file count; children are file cards laid out in a responsive grid.
// Edges are dotted, no arrows, connecting bottom-middle of the source to
// top-middle of the target via smoothstep.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	Background,
	BackgroundVariant,
	Controls,
	MiniMap,
	ReactFlow,
	ReactFlowProvider,
	type Edge,
	type Node,
	type NodeMouseHandler,
	type NodeTypes,
	useReactFlow,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import dagre from "@dagrejs/dagre";
import { FileNodeCard } from "./FileNodeCard";
import { GroupContainer, DEFAULT_GROUP_COLOR, type GroupContainerData } from "./GroupContainer";
import type { FileGroup } from "./componentGrouper";
import {
	loadCollapsed,
	loadGroupOverrides,
	loadNodeColors,
	saveCollapsed,
	saveGroupOverride,
	saveNodeColor,
	type GroupOverrides,
	type NodeColorOverrides,
} from "./mermaidOverrides";
import { type BuildResult, type FileEdgeData, type FileNodeData } from "./mermaidGraphBuilder";
import type { BulkNode } from "./types";

interface Props {
	projectId: string;
	graph: BuildResult;
	selectedNode: BulkNode | null;
	onSelectFile: (file: BulkNode | null) => void;
}

// Geometry constants — tuned so groups breathe and don't visually collide.
const NODE_WIDTH = 220;
const NODE_HEIGHT = 130;
// Grid gaps only used for the isolated-files bucket (no edges, so no dagre).
const COL_GAP = 28;
const ROW_GAP = 32;
// Dagre spacing inside connected groups. Larger than the grid gaps because
// hierarchical layouts need vertical room for edges to sweep between ranks.
const DAGRE_NODE_SEP = 70;
const DAGRE_RANK_SEP = 110;
const GROUP_HEADER = 44;
const GROUP_PAD_X = 28;
const GROUP_PAD_TOP = 28;
const GROUP_PAD_BOTTOM = 28;
const GROUP_GAP_X = 120;
const GROUP_GAP_Y = 140;
const MARGIN_X = 80;
const MARGIN_Y = 60;
// Row-wrap threshold for the 2D group flow. Groups pack left-to-right
// until the next group would exceed this, then wrap to a new row.
const MAX_ROW_WIDTH = 2800;

const EDGE_COLORS: Record<string, string> = {
	IMPORTS: "rgba(147,197,253,0.85)",
	CALLS: "rgba(252,211,77,0.85)",
	EXTENDS: "rgba(251,146,60,0.85)",
	IMPLEMENTS: "rgba(196,181,253,0.85)",
};
const DEFAULT_EDGE_COLOR = "rgba(212,165,116,0.75)";

function colsForGroup(n: number): number {
	if (n <= 4) return Math.max(1, n);
	const byRoot = Math.ceil(Math.sqrt(n));
	return Math.max(4, Math.min(8, byRoot));
}

interface LayoutInput {
	visibleGroups: FileGroup[];
	fileNodes: Node<FileNodeData>[];
	fileDegree: Map<string, number>;
	// Edges that stay inside a single visible group (used for dagre).
	intraEdges: Edge<FileEdgeData>[];
	// Set of group IDs that should render as summary tiles (no children).
	collapsedGroupIds: Set<string>;
	groupOverrides: GroupOverrides;
	nodeColors: NodeColorOverrides;
	onRenameGroup: (groupId: string, title: string | null) => void;
	onRecolorGroup: (groupId: string, color: string | null) => void;
	onRecolorNode: (fileQname: string, color: string | null) => void;
	onToggleCollapsed: (groupId: string) => void;
}

// When the project has more files than this threshold, every group starts
// collapsed; the user expands what they want to inspect. Dramatically cuts
// initial render cost since only group container tiles render up front.
const COLLAPSE_BY_DEFAULT_THRESHOLD = 100;
// Size of a collapsed group tile (header-only plus a small footer hint).
const COLLAPSED_GROUP_WIDTH = 280;
const COLLAPSED_GROUP_HEIGHT = 72;

// A graph becomes "huge" at this node count; we skip MiniMap and
// fitView animation to reduce the first-paint cost.
const HUGE_GRAPH_NODES = 500;

// One aggregated edge spanning two groups. Every individual cross-group
// IMPORTS/CALLS/EXTENDS/IMPLEMENTS collapses into a single bundle so
// the canvas renders tens of lines instead of thousands.
interface CrossGroupBundle {
	id: string;
	sourceGroupId: string;
	targetGroupId: string;
	count: number;
	types: Record<string, number>;
}

function classifyEdges(
	edges: Edge<FileEdgeData>[],
	fileToGroup: Map<string, string>,
	collapsedGroupIds: Set<string>,
): { intra: Edge<FileEdgeData>[]; bundles: CrossGroupBundle[] } {
	const intra: Edge<FileEdgeData>[] = [];
	const bundleMap = new Map<string, CrossGroupBundle>();
	for (const e of edges) {
		const sg = fileToGroup.get(e.source);
		const dg = fileToGroup.get(e.target);
		// Both endpoints must map to a visible group.
		if (!sg || !dg) continue;
		if (sg === dg) {
			// Skip intra-group edges for collapsed groups — the child
			// cards aren't rendered so there's nothing to connect.
			if (collapsedGroupIds.has(sg)) continue;
			intra.push(e);
			continue;
		}
		const key = `${sg}\u0001${dg}`;
		const count = e.data?.count ?? 1;
		const type = e.data?.edgeType ?? "";
		const bundle = bundleMap.get(key) ?? {
			id: `bundle:${sg}__${dg}`,
			sourceGroupId: sg,
			targetGroupId: dg,
			count: 0,
			types: {},
		};
		bundle.count += count;
		bundle.types[type] = (bundle.types[type] ?? 0) + count;
		bundleMap.set(key, bundle);
	}
	return { intra, bundles: Array.from(bundleMap.values()) };
}

interface LocalLayout {
	// Map fileQname → top-left within the group content area (already
	// offset for the title bar + padding).
	positions: Map<string, { x: number; y: number }>;
	innerW: number;
	innerH: number;
}

// Hierarchical layout via dagre using only the edges internal to the
// group. Nodes with more callers stack above nodes they call, producing
// a top-down DAG that edges can follow without cutting across cards.
function layoutGroupDagre(
	fileQnames: string[],
	groupEdges: Array<{ source: string; target: string }>,
): LocalLayout {
	const g = new dagre.graphlib.Graph({ compound: false });
	g.setGraph({
		rankdir: "TB",
		nodesep: DAGRE_NODE_SEP,
		ranksep: DAGRE_RANK_SEP,
		marginx: 0,
		marginy: 0,
		ranker: "tight-tree",
	});
	g.setDefaultEdgeLabel(() => ({}));

	for (const qname of fileQnames) {
		g.setNode(qname, { width: NODE_WIDTH, height: NODE_HEIGHT });
	}
	const seen = new Set<string>();
	for (const e of groupEdges) {
		const key = `${e.source}\u0001${e.target}`;
		if (seen.has(key)) continue;
		seen.add(key);
		g.setEdge(e.source, e.target);
	}
	dagre.layout(g);

	let minX = Infinity;
	let minY = Infinity;
	let maxX = -Infinity;
	let maxY = -Infinity;
	const raw = new Map<string, { x: number; y: number }>();
	for (const qname of fileQnames) {
		const n = g.node(qname);
		if (!n) continue;
		// dagre returns node center — convert to top-left.
		const left = n.x - NODE_WIDTH / 2;
		const top = n.y - NODE_HEIGHT / 2;
		raw.set(qname, { x: left, y: top });
		minX = Math.min(minX, left);
		minY = Math.min(minY, top);
		maxX = Math.max(maxX, left + NODE_WIDTH);
		maxY = Math.max(maxY, top + NODE_HEIGHT);
	}
	if (raw.size === 0) return { positions: new Map(), innerW: NODE_WIDTH, innerH: NODE_HEIGHT };

	// Translate so the layout starts at (0, 0), then offset into the
	// group's inner content area.
	const positions = new Map<string, { x: number; y: number }>();
	for (const [qname, p] of raw) {
		positions.set(qname, {
			x: GROUP_PAD_X + (p.x - minX),
			y: GROUP_HEADER + GROUP_PAD_TOP + (p.y - minY),
		});
	}
	return {
		positions,
		innerW: Math.max(0, maxX - minX),
		innerH: Math.max(0, maxY - minY),
	};
}

// Compact grid for the isolated-files bucket (no edges to arrange).
function layoutGroupGrid(fileQnames: string[]): LocalLayout {
	const cols = colsForGroup(fileQnames.length);
	const rows = Math.ceil(fileQnames.length / cols);
	const positions = new Map<string, { x: number; y: number }>();
	fileQnames.forEach((qname, i) => {
		const col = i % cols;
		const row = Math.floor(i / cols);
		positions.set(qname, {
			x: GROUP_PAD_X + col * (NODE_WIDTH + COL_GAP),
			y: GROUP_HEADER + GROUP_PAD_TOP + row * (NODE_HEIGHT + ROW_GAP),
		});
	});
	return {
		positions,
		innerW: cols * NODE_WIDTH + (cols - 1) * COL_GAP,
		innerH: rows * NODE_HEIGHT + (rows - 1) * ROW_GAP,
	};
}

// Build the full (parent + child) node list. Parents are GroupContainer
// nodes sized to fit their (dagre-laid-out) child FileNodeCards; children
// use `parentId` so React Flow places them relative to the container.
// Groups pack left-to-right and wrap to a new row when they'd exceed
// MAX_ROW_WIDTH so the layout spreads in 2D rather than one long column.
function buildLayoutNodes(input: LayoutInput): Node[] {
	const { visibleGroups, fileNodes, intraEdges, collapsedGroupIds, groupOverrides, nodeColors,
		onRenameGroup, onRecolorGroup, onRecolorNode, onToggleCollapsed } = input;
	const fileNodesByQname = new Map<string, Node<FileNodeData>>();
	for (const n of fileNodes) fileNodesByQname.set(n.id, n);

	// Map file → group so we can pick out per-group edges cheaply.
	const fileToGroup = new Map<string, string>();
	for (const group of visibleGroups) {
		for (const f of group.files) fileToGroup.set(f.qualified_name, group.id);
	}
	const edgesByGroup = new Map<string, Array<{ source: string; target: string }>>();
	for (const e of intraEdges) {
		const gs = fileToGroup.get(e.source);
		if (!gs) continue;
		const arr = edgesByGroup.get(gs) ?? [];
		arr.push({ source: e.source, target: e.target });
		edgesByGroup.set(gs, arr);
	}

	interface Placed {
		group: FileGroup;
		layout: LocalLayout;
		width: number;
		height: number;
		x: number;
		y: number;
	}
	const placedGroups: Placed[] = [];
	let rowX = MARGIN_X;
	let rowY = MARGIN_Y;
	let rowH = 0;

	for (const group of visibleGroups) {
		if (group.files.length === 0) continue;

		const isCollapsed = collapsedGroupIds.has(group.id);
		let layout: LocalLayout;
		let groupW: number;
		let groupH: number;

		if (isCollapsed) {
			// No dagre, no file positions — just a compact summary tile.
			layout = { positions: new Map(), innerW: 0, innerH: 0 };
			groupW = COLLAPSED_GROUP_WIDTH;
			groupH = COLLAPSED_GROUP_HEIGHT;
		} else {
			const qnames = group.files.map((f) => f.qualified_name);
			// Tiny groups skip dagre — each dagre call has fixed overhead
			// (object allocation + ranker setup) that dominates layout time
			// for 2-to-4 file clusters.
			const useDagre = !group.isolated && group.files.length > 4;
			layout = useDagre
				? layoutGroupDagre(qnames, edgesByGroup.get(group.id) ?? [])
				: layoutGroupGrid(qnames);
			groupW = layout.innerW + GROUP_PAD_X * 2;
			groupH = layout.innerH + GROUP_HEADER + GROUP_PAD_TOP + GROUP_PAD_BOTTOM;
		}

		if (rowX > MARGIN_X && rowX + groupW > MARGIN_X + MAX_ROW_WIDTH) {
			rowX = MARGIN_X;
			rowY += rowH + GROUP_GAP_Y;
			rowH = 0;
		}

		placedGroups.push({ group, layout, width: groupW, height: groupH, x: rowX, y: rowY });
		rowX += groupW + GROUP_GAP_X;
		rowH = Math.max(rowH, groupH);
	}

	const out: Node[] = [];
	for (const p of placedGroups) {
		const override = groupOverrides[p.group.id];
		const title = override?.title ?? p.group.defaultTitle;
		const color = override?.color ?? DEFAULT_GROUP_COLOR;

		const groupData: GroupContainerData = {
			groupId: p.group.id,
			title,
			color,
			fileCount: p.group.files.length,
			collapsed: collapsedGroupIds.has(p.group.id),
			onRenameGroup,
			onRecolorGroup,
			onToggleCollapsed,
		};

		out.push({
			id: p.group.id,
			type: "groupContainer",
			position: { x: p.x, y: p.y },
			width: p.width,
			height: p.height,
			data: groupData as unknown as Record<string, unknown>,
			selectable: false,
			draggable: false,
		});

		for (const file of p.group.files) {
			const base = fileNodesByQname.get(file.qualified_name);
			const pos = p.layout.positions.get(file.qualified_name);
			if (!base || !pos) continue;
			const overrideColor = nodeColors[file.qualified_name];
			out.push({
				...base,
				parentId: p.group.id,
				extent: "parent",
				position: pos,
				width: NODE_WIDTH,
				height: NODE_HEIGHT,
				data: {
					...base.data,
					colorOverride: overrideColor,
					onRecolorNode,
				},
			});
		}
	}

	return out;
}

// Dotted bezier edges, no arrowheads. Width encodes aggregated count.
// Paths are selection-aware: when a node is focused, unrelated edges
// fade out so the relevant relationships pop.
function styleEdges(edges: Edge<FileEdgeData>[], selectedId: string | null): Edge<FileEdgeData>[] {
	return edges.map((edge) => {
		const type = edge.data?.edgeType ?? "";
		const count = edge.data?.count ?? 1;
		const color = EDGE_COLORS[type] ?? DEFAULT_EDGE_COLOR;
		const strokeWidth = Math.min(1.5 + Math.log2(count + 1), 3.5);
		const isRelated = selectedId != null && (edge.source === selectedId || edge.target === selectedId);
		const dim = selectedId != null && !isRelated;
		return {
			...edge,
			type: "default", // bezier — curves naturally when going left/right
			animated: false,
			focusable: false,
			style: {
				stroke: color,
				strokeWidth: isRelated ? strokeWidth + 1 : strokeWidth,
				strokeDasharray: "2 4",
				strokeLinecap: "round",
				opacity: dim ? 0.12 : 1,
			},
			markerEnd: undefined,
		};
	});
}

// Thick solid-looking bundle edges between group containers. One per
// (sourceGroup, targetGroup) pair regardless of how many underlying
// file-level edges they represent. Count shown as a label.
function styleBundles(
	bundles: CrossGroupBundle[],
	selectedGroupIds: Set<string> | null,
): Edge<FileEdgeData>[] {
	return bundles.map((b) => {
		// Pick the dominant type's color for visual distinction.
		let dominant: string | null = null;
		let max = 0;
		for (const [type, n] of Object.entries(b.types)) {
			if (n > max) { max = n; dominant = type; }
		}
		const color = (dominant && EDGE_COLORS[dominant]) ?? DEFAULT_EDGE_COLOR;
		const strokeWidth = Math.min(2 + Math.log2(b.count + 1), 6);
		const isRelated = selectedGroupIds != null
			&& (selectedGroupIds.has(b.sourceGroupId) || selectedGroupIds.has(b.targetGroupId));
		const dim = selectedGroupIds != null && !isRelated;
		return {
			id: b.id,
			source: b.sourceGroupId,
			target: b.targetGroupId,
			data: { edgeType: dominant ?? "bundle", count: b.count },
			type: "default",
			animated: false,
			focusable: false,
			selectable: false,
			label: b.count > 1 ? String(b.count) : undefined,
			labelStyle: { fill: "rgba(220,220,235,0.95)", fontSize: 11, fontFamily: "ui-monospace, monospace" },
			labelBgStyle: { fill: "rgba(20,20,30,0.9)" },
			labelBgPadding: [4, 2] as [number, number],
			labelBgBorderRadius: 4,
			style: {
				stroke: color,
				strokeWidth,
				strokeLinecap: "round",
				opacity: dim ? 0.15 : 0.55,
			},
			markerEnd: undefined,
		};
	});
}

const NODE_TYPES: NodeTypes = { file: FileNodeCard, groupContainer: GroupContainer };

function InnerFlow({ projectId, graph, selectedNode, onSelectFile }: Props) {
	const { nodes: fileRfNodes, edges: builtEdges, totalFiles, fileDegree, groups } = graph;
	const selectedId = selectedNode?.label === "File" ? selectedNode.qualified_name : null;

	const [groupOverrides, setGroupOverrides] = useState<GroupOverrides>(() => loadGroupOverrides(projectId));
	const [nodeColors, setNodeColors] = useState<NodeColorOverrides>(() => loadNodeColors(projectId));
	// Hide the isolated-files bucket by default — on big projects it's
	// often 70%+ of the cards and kills first-paint performance.
	const [showIsolated, setShowIsolated] = useState(false);

	// Collapsed state. On large projects every group starts collapsed —
	// only a few dozen container tiles render up front, users expand
	// what they want. Per-group explicit toggles override the default.
	const [collapsedExplicit, setCollapsedExplicit] = useState<Record<string, boolean>>(
		() => loadCollapsed(projectId).explicit,
	);
	const defaultCollapsed = totalFiles > COLLAPSE_BY_DEFAULT_THRESHOLD;
	const collapsedGroupIds = useMemo(() => {
		const s = new Set<string>();
		for (const g of groups) {
			const explicit = collapsedExplicit[g.id];
			const isCollapsed = explicit !== undefined ? explicit : defaultCollapsed;
			if (isCollapsed) s.add(g.id);
		}
		return s;
	}, [groups, collapsedExplicit, defaultCollapsed]);

	const isolatedCount = useMemo(
		() => groups.filter((g) => g.isolated).reduce((n, g) => n + g.files.length, 0),
		[groups],
	);

	// Reload overrides when the project changes.
	useEffect(() => {
		setGroupOverrides(loadGroupOverrides(projectId));
		setNodeColors(loadNodeColors(projectId));
		setCollapsedExplicit(loadCollapsed(projectId).explicit);
	}, [projectId]);

	const handleToggleCollapsed = useCallback((groupId: string) => {
		setCollapsedExplicit((prev) => {
			// Toggle against the current effective state (explicit or default).
			const currentlyCollapsed = prev[groupId] !== undefined ? prev[groupId] : defaultCollapsed;
			const next = { ...prev, [groupId]: !currentlyCollapsed };
			saveCollapsed(projectId, { explicit: next });
			return next;
		});
	}, [projectId, defaultCollapsed]);

	const handleRenameGroup = useCallback((groupId: string, title: string | null) => {
		setGroupOverrides((prev) => {
			const next = { ...prev };
			const cur = next[groupId] ?? {};
			if (title === null || title === "") {
				const { title: _t, ...rest } = cur;
				if (Object.keys(rest).length === 0) delete next[groupId];
				else next[groupId] = rest;
			} else {
				next[groupId] = { ...cur, title };
			}
			saveGroupOverride(projectId, groupId, { title: title ?? undefined });
			return next;
		});
	}, [projectId]);

	const handleRecolorGroup = useCallback((groupId: string, color: string | null) => {
		setGroupOverrides((prev) => {
			const next = { ...prev };
			const cur = next[groupId] ?? {};
			if (color === null) {
				const { color: _c, ...rest } = cur;
				if (Object.keys(rest).length === 0) delete next[groupId];
				else next[groupId] = rest;
			} else {
				next[groupId] = { ...cur, color };
			}
			saveGroupOverride(projectId, groupId, { color: color ?? undefined });
			return next;
		});
	}, [projectId]);

	const handleRecolorNode = useCallback((fileQname: string, color: string | null) => {
		setNodeColors((prev) => {
			const next = { ...prev };
			if (color === null) delete next[fileQname];
			else next[fileQname] = color;
			saveNodeColor(projectId, fileQname, color);
			return next;
		});
	}, [projectId]);

	const visibleGroups = useMemo(
		() => (showIsolated ? groups : groups.filter((g) => !g.isolated)),
		[groups, showIsolated],
	);

	// File → group for every visible file (used for edge classification).
	const fileToGroup = useMemo(() => {
		const m = new Map<string, string>();
		for (const g of visibleGroups) for (const f of g.files) m.set(f.qualified_name, g.id);
		return m;
	}, [visibleGroups]);

	// Split edges once into intra-group (fine-grained, rendered as dotted
	// bezier between file cards) and cross-group bundles (one thick edge
	// per group pair, labeled with the count). Cross-group edges used to
	// dominate the SVG path count on large projects — bundling turns
	// thousands of paths into dozens.
	const { intra: intraEdges, bundles: crossBundles } = useMemo(
		() => classifyEdges(builtEdges, fileToGroup, collapsedGroupIds),
		[builtEdges, fileToGroup, collapsedGroupIds],
	);

	const laidOutNodes = useMemo(
		() => buildLayoutNodes({
			visibleGroups,
			fileNodes: fileRfNodes,
			fileDegree,
			intraEdges,
			collapsedGroupIds,
			groupOverrides,
			nodeColors,
			onRenameGroup: handleRenameGroup,
			onRecolorGroup: handleRecolorGroup,
			onRecolorNode: handleRecolorNode,
			onToggleCollapsed: handleToggleCollapsed,
		}),
		[visibleGroups, fileRfNodes, fileDegree, intraEdges, collapsedGroupIds, groupOverrides, nodeColors,
			handleRenameGroup, handleRecolorGroup, handleRecolorNode, handleToggleCollapsed],
	);

	// Selected group set — drives bundle dimming so only the focused
	// file's cross-group bundles stay prominent.
	const selectedGroupIds = useMemo<Set<string> | null>(() => {
		if (!selectedId) return null;
		const sg = fileToGroup.get(selectedId);
		return sg ? new Set([sg]) : new Set();
	}, [selectedId, fileToGroup]);

	const displayEdges = useMemo(
		() => [
			...styleEdges(intraEdges, selectedId),
			...styleBundles(crossBundles, selectedGroupIds),
		],
		[intraEdges, crossBundles, selectedId, selectedGroupIds],
	);
	const isHuge = laidOutNodes.length > HUGE_GRAPH_NODES;

	// Auto-expand the group containing the currently-selected file so
	// the user always sees what they clicked on.
	useEffect(() => {
		if (!selectedId) return;
		const sg = fileToGroup.get(selectedId);
		if (!sg) return;
		if (!collapsedGroupIds.has(sg)) return;
		setCollapsedExplicit((prev) => {
			const next = { ...prev, [sg]: false };
			saveCollapsed(projectId, { explicit: next });
			return next;
		});
	}, [selectedId, fileToGroup, collapsedGroupIds, projectId]);

	const expandAll = useCallback(() => {
		const next: Record<string, boolean> = {};
		for (const g of groups) next[g.id] = false;
		setCollapsedExplicit(next);
		saveCollapsed(projectId, { explicit: next });
	}, [groups, projectId]);

	const collapseAll = useCallback(() => {
		const next: Record<string, boolean> = {};
		for (const g of groups) next[g.id] = true;
		setCollapsedExplicit(next);
		saveCollapsed(projectId, { explicit: next });
	}, [groups, projectId]);

	const { fitView, setNodes, setEdges, updateNode } = useReactFlow<Node, Edge<FileEdgeData>>();

	const prevSelectedIdRef = useRef<string | null>(null);
	useEffect(() => {
		setNodes(
			laidOutNodes.map((n) => (n.id === selectedId ? { ...n, selected: true } : n)),
		);
		prevSelectedIdRef.current = selectedId;
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [laidOutNodes, setNodes]);

	useEffect(() => {
		setEdges(displayEdges);
	}, [displayEdges, setEdges]);

	useEffect(() => {
		const prev = prevSelectedIdRef.current;
		if (prev === selectedId) return;
		if (prev) updateNode(prev, { selected: false });
		if (selectedId) updateNode(selectedId, { selected: true });
		prevSelectedIdRef.current = selectedId;
	}, [selectedId, updateNode]);

	// Fit view once per layout change (which only happens when the
	// underlying graph / groups change, not on hover or selection).
	// On huge graphs skip the animation — instant snap is cheaper.
	const layoutSignatureRef = useRef<string>("");
	useEffect(() => {
		const sig = `${groups.length}:${fileRfNodes.length}:${showIsolated}`;
		if (layoutSignatureRef.current === sig) return;
		layoutSignatureRef.current = sig;
		const id = requestAnimationFrame(() => {
			fitView({ padding: 0.15, duration: isHuge ? 0 : 200 });
		});
		return () => cancelAnimationFrame(id);
	}, [groups.length, fileRfNodes.length, showIsolated, fitView, isHuge]);

	const onNodeClick: NodeMouseHandler = useCallback((_event, node) => {
		if (node.type !== "file") return;
		const data = node.data as FileNodeData | undefined;
		if (data?.file) onSelectFile(data.file);
	}, [onSelectFile]);

	const onPaneClick = useCallback(() => {
		if (selectedNode) onSelectFile(null);
	}, [selectedNode, onSelectFile]);

	const onNodesChange = useCallback(() => { /* positions static */ }, []);

	return (
		<div className="relative flex h-full w-full flex-col bg-app">
			<div className="flex items-center gap-3 border-b border-app-line bg-app-darkBox px-4 py-1.5 text-[10px] text-ink-faint">
				<span>
					{totalFiles.toLocaleString()} {totalFiles === 1 ? "file" : "files"} ·{" "}
					{visibleGroups.length.toLocaleString()} {visibleGroups.length === 1 ? "group" : "groups"} ·{" "}
					{intraEdges.length.toLocaleString()} intra ·{" "}
					{crossBundles.length.toLocaleString()} cross-group {crossBundles.length === 1 ? "bundle" : "bundles"}
				</span>
				{isolatedCount > 0 && (
					<button
						type="button"
						onClick={() => setShowIsolated((v) => !v)}
						className="rounded border border-app-line px-2 py-0.5 text-ink-dull transition-colors hover:border-accent hover:text-ink"
						title="Isolated files have no cross-file IMPORTS/CALLS/EXTENDS/IMPLEMENTS relationships"
					>
						{showIsolated ? `Hide isolated (${isolatedCount.toLocaleString()})` : `Show isolated (${isolatedCount.toLocaleString()})`}
					</button>
				)}
				<button
					type="button"
					onClick={expandAll}
					className="rounded border border-app-line px-2 py-0.5 text-ink-dull transition-colors hover:border-accent hover:text-ink"
					title="Expand every group"
				>
					Expand all
				</button>
				<button
					type="button"
					onClick={collapseAll}
					className="rounded border border-app-line px-2 py-0.5 text-ink-dull transition-colors hover:border-accent hover:text-ink"
					title="Collapse every group"
				>
					Collapse all
				</button>
				<span className="ml-auto">Click a file to inspect · Chevron toggles a group · Scroll to zoom</span>
			</div>

			<div className="relative flex-1">
				<ReactFlow
					defaultNodes={laidOutNodes}
					defaultEdges={displayEdges}
					onNodesChange={onNodesChange}
					onNodeClick={onNodeClick}
					onPaneClick={onPaneClick}
					nodeTypes={NODE_TYPES}
					nodesDraggable={false}
					nodesConnectable={false}
					elementsSelectable
					edgesFocusable={false}
					onlyRenderVisibleElements
					proOptions={{ hideAttribution: true }}
					colorMode="dark"
					minZoom={0.05}
					maxZoom={2}
					fitView
					fitViewOptions={{ padding: 0.15 }}
				>
					<Background variant={BackgroundVariant.Dots} gap={20} size={1} color="rgba(180,180,200,0.12)" />
					<Controls showInteractive={false} />
					{!isHuge && (
						<MiniMap
							pannable
							zoomable
							nodeStrokeWidth={2}
							nodeColor={(n) => {
								if (n.type === "groupContainer") {
									const d = n.data as unknown as GroupContainerData;
									return `${d.color ?? DEFAULT_GROUP_COLOR}55`;
								}
								const d = n.data as FileNodeData & { colorOverride?: string };
								return d?.colorOverride ?? "#3b82f6";
							}}
							maskColor="rgba(10,10,15,0.7)"
							style={{ background: "rgba(20,20,30,0.9)" }}
						/>
					)}
				</ReactFlow>
			</div>
		</div>
	);
}

export function MermaidView(props: Props) {
	return (
		<ReactFlowProvider>
			<InnerFlow {...props} />
		</ReactFlowProvider>
	);
}
