// Converts the bulk-node/bulk-edge payloads into a graphology Graph that
// Sigma can render. Applies hierarchy-based initial positioning (folders
// spread out, children near their parents) and community-based coloring
// for symbol nodes.
//
// Ported from reference/GitNexus/gitnexus-web/src/lib/graph-adapter.ts and
// adapted to spacebot's node shape:
//   - numeric `id` per label table (NOT globally unique — composite
//     `label:id` keys are used in graphology)
//   - `source_file` instead of GitNexus's `properties.filePath`
//   - top-level `name` / `line_start` / `line_end`
//   - extra labels (Struct/Trait/Impl/Community/Process/...)

import Graph from "graphology";
import { NODE_COLORS, NODE_SIZES, getCommunityColor, type NodeLabel, type EdgeType } from "./constants";
import type { BulkNode, BulkEdge } from "./types";

export interface SigmaNodeAttributes {
	x: number;
	y: number;
	size: number;
	color: string;
	label: string;
	nodeType: NodeLabel;
	sourceFile: string | null;
	lineStart: number | null;
	lineEnd: number | null;
	nodeId: number;
	hidden?: boolean;
	zIndex?: number;
	highlighted?: boolean;
	mass?: number;
	community?: number;
}

export interface SigmaEdgeAttributes {
	size: number;
	color: string;
	relationType: EdgeType | string;
	type?: string;
}

/** Format bytes into a human-readable string (e.g. 1.2 KB, 3.4 MB). */
const formatFileSize = (bytes: number): string => {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

const STRUCTURAL_LABELS = new Set<NodeLabel>([
	"Project",
	"Package",
	"Module",
	"Namespace",
	"Folder",
]);

const SYMBOL_LABELS = new Set<NodeLabel>([
	"Function",
	"Method",
	"Class",
	"Interface",
	"Struct",
	"Trait",
	"Impl",
	"Enum",
	"Type",
	"TypeAlias",
	"Const",
	"MacroDef",
	"Record",
	"Template",
]);

// Metadata labels never drawn as regular nodes — we still add them to the
// graph so MEMBER_OF / STEP_IN_PROCESS edges have both endpoints, but they
// render at size 0.
const INVISIBLE_LABELS = new Set<NodeLabel>(["Community", "Process"]);

const HIERARCHY_RELATIONS = new Set<string>(["CONTAINS", "DEFINES", "IMPORTS"]);

/** Scale down node sizes on large graphs so they don't drown each other out. */
const getScaledNodeSize = (baseSize: number, nodeCount: number): number => {
	if (nodeCount > 20000) return Math.max(1.5, baseSize * 0.5);
	if (nodeCount > 5000) return Math.max(2, baseSize * 0.65);
	if (nodeCount > 1000) return Math.max(2.5, baseSize * 0.8);
	return baseSize;
};

/** ForceAtlas2 mass — higher = more repulsion. Folders push outward. */
const getNodeMass = (label: NodeLabel, nodeCount: number): number => {
	const mult = nodeCount > 5000 ? 2 : nodeCount > 1000 ? 1.5 : 1;
	switch (label) {
		case "Project":
			return 50 * mult;
		case "Package":
			return 30 * mult;
		case "Module":
		case "Namespace":
			return 20 * mult;
		case "Folder":
			return 15 * mult;
		case "File":
			return 3 * mult;
		case "Class":
		case "Interface":
		case "Struct":
		case "Trait":
		case "Record":
			return 5 * mult;
		case "Function":
		case "Method":
			return 2 * mult;
		default:
			return 1;
	}
};

// ---------------------------------------------------------------------------
// Key helpers — LadybugDB `id(n)` returns 0 for all nodes, so we use
// `qualified_name` as the unique graphology key instead. The bulk-edge
// endpoint also returns `from_qname` / `to_qname` (qualified names).
// ---------------------------------------------------------------------------

/** Graphology key for a node — its qualified_name. */
export const nodeKey = (n: BulkNode): string => n.qualified_name;

/** Graphology key for the source side of a bulk edge. */
const ekSrc = (e: BulkEdge): string => e.from_qname;

/** Graphology key for the target side of a bulk edge. */
const ekTgt = (e: BulkEdge): string => e.to_qname;

// ---------------------------------------------------------------------------
// Main conversion
// ---------------------------------------------------------------------------

/** Resolve a node's display color, respecting user overrides. */
export const getNodeColor = (
	label: string,
	colorOverrides?: Record<string, string>,
): string =>
	colorOverrides?.[label] ?? NODE_COLORS[label as NodeLabel] ?? "#6b7280";

export const buildGraph = (
	bulkNodes: BulkNode[],
	bulkEdges: BulkEdge[],
	colorOverrides?: Record<string, string>,
): Graph<SigmaNodeAttributes, SigmaEdgeAttributes> => {
	const graph = new Graph<SigmaNodeAttributes, SigmaEdgeAttributes>();
	const nodeCount = bulkNodes.length;

	// Coerce a potentially-unknown label string into a NodeLabel. Unknown
	// labels fall through to a default color/size pair below.
	const asNodeLabel = (label: string): NodeLabel => label as NodeLabel;

	// Build parent → children map from CONTAINS/DEFINES/IMPORTS edges.
	const parentToChildren = new Map<string, string[]>();
	const childToParent = new Map<string, string>();

	// Build community memberships from MEMBER_OF edges. The target of
	// MEMBER_OF is a Community node; we remember that mapping for coloring.
	const memberCommunity = new Map<string, number>();
	const communityIdByNodeId = new Map<string, number>();

	// First pass over nodes to assign community indices: Community nodes
	// are assigned dense numeric indices based on insertion order.
	const communityNodes = bulkNodes.filter((n) => n.label === "Community");
	communityNodes.forEach((n, i) => {
		communityIdByNodeId.set(nodeKey(n), i);
	});

	for (const rel of bulkEdges) {
		const src = ekSrc(rel);
		const tgt = ekTgt(rel);
		if (HIERARCHY_RELATIONS.has(rel.edge_type)) {
			if (!parentToChildren.has(src)) parentToChildren.set(src, []);
			parentToChildren.get(src)!.push(tgt);
			childToParent.set(tgt, src);
		}
		if (rel.edge_type === "MEMBER_OF") {
			const communityIdx = communityIdByNodeId.get(tgt);
			if (communityIdx !== undefined) {
				memberCommunity.set(src, communityIdx);
			}
		}
	}

	const nodeByKey = new Map<string, BulkNode>();
	bulkNodes.forEach((n) => nodeByKey.set(nodeKey(n), n));

	const structuralNodes = bulkNodes.filter((n) =>
		STRUCTURAL_LABELS.has(asNodeLabel(n.label)),
	);

	// Wide spread for top-level structural nodes.
	const structuralSpread = Math.sqrt(Math.max(nodeCount, 1)) * 40;
	const childJitter = Math.sqrt(Math.max(nodeCount, 1)) * 3;
	const clusterJitter = Math.sqrt(Math.max(nodeCount, 1)) * 1.5;

	// Compute cluster centers (one per community) in a golden-angle spiral
	// so communities land in roughly evenly-distributed regions.
	const clusterCenters = new Map<number, { x: number; y: number }>();
	if (memberCommunity.size > 0) {
		const communities = new Set(memberCommunity.values());
		const count = communities.size;
		const goldenAngle = Math.PI * (3 - Math.sqrt(5));
		let idx = 0;
		for (const c of communities) {
			const angle = idx * goldenAngle;
			const radius = structuralSpread * 0.8 * Math.sqrt((idx + 1) / Math.max(count, 1));
			clusterCenters.set(c, {
				x: radius * Math.cos(angle),
				y: radius * Math.sin(angle),
			});
			idx++;
		}
	}

	const nodePositions = new Map<string, { x: number; y: number }>();

	// Helper that pushes a node into graphology with all attributes.
	const addNode = (id: string, x: number, y: number): void => {
		const node = nodeByKey.get(id);
		if (!node) return;
		const label = asNodeLabel(node.label);
		const isInvisible = INVISIBLE_LABELS.has(label);
		const baseSize = isInvisible ? 0 : NODE_SIZES[label] ?? 6;
		const scaledSize = isInvisible ? 0 : getScaledNodeSize(baseSize, nodeCount);

		const community = memberCommunity.get(id);
		const useCommunityColor = community !== undefined && SYMBOL_LABELS.has(label) && !colorOverrides?.[label];
		const color = useCommunityColor
			? getCommunityColor(community!)
			: getNodeColor(label, colorOverrides);

		// File labels show the file size so users can gauge weight at a
		// glance (e.g. "server.rs (12.3 KB)").
		const displayLabel =
			label === "File" && node.file_size
				? `${node.name} (${formatFileSize(node.file_size)})`
				: node.name;

		graph.addNode(id, {
			x,
			y,
			size: scaledSize,
			color,
			label: displayLabel,
			nodeType: label,
			sourceFile: node.source_file ?? null,
			lineStart: (node.line_start ?? null) as number | null,
			lineEnd: (node.line_end ?? null) as number | null,
			nodeId: node.id,
			hidden: isInvisible,
			mass: getNodeMass(label, nodeCount),
			community,
		});
	};

	// 1. Position structural nodes in a golden-angle spiral.
	const goldenAngle = Math.PI * (3 - Math.sqrt(5));
	structuralNodes.forEach((node, index) => {
		const angle = index * goldenAngle;
		const radius =
			structuralSpread * Math.sqrt((index + 1) / Math.max(structuralNodes.length, 1));
		const jitter = structuralSpread * 0.15;
		const x = radius * Math.cos(angle) + (Math.random() - 0.5) * jitter;
		const y = radius * Math.sin(angle) + (Math.random() - 0.5) * jitter;
		const id = nodeKey(node);
		nodePositions.set(id, { x, y });
		addNode(id, x, y);
	});

	// 2. BFS from structural nodes: each child lands near its parent (or,
	//    for symbol nodes with a community, near the cluster center).
	const queue: string[] = structuralNodes.map((n) => nodeKey(n));
	const visited = new Set<string>(queue);

	while (queue.length > 0) {
		const currentId = queue.shift()!;
		const children = parentToChildren.get(currentId) ?? [];
		for (const childId of children) {
			if (visited.has(childId)) continue;
			visited.add(childId);

			const child = nodeByKey.get(childId);
			if (!child) continue;
			const childLabel = asNodeLabel(child.label);

			let x: number;
			let y: number;
			const community = memberCommunity.get(childId);
			const clusterCenter =
				community !== undefined ? clusterCenters.get(community) : undefined;
			if (clusterCenter && SYMBOL_LABELS.has(childLabel)) {
				x = clusterCenter.x + (Math.random() - 0.5) * clusterJitter;
				y = clusterCenter.y + (Math.random() - 0.5) * clusterJitter;
			} else {
				const parentPos = nodePositions.get(currentId);
				if (parentPos) {
					x = parentPos.x + (Math.random() - 0.5) * childJitter;
					y = parentPos.y + (Math.random() - 0.5) * childJitter;
				} else {
					x = (Math.random() - 0.5) * structuralSpread * 0.5;
					y = (Math.random() - 0.5) * structuralSpread * 0.5;
				}
			}
			nodePositions.set(childId, { x, y });
			addNode(childId, x, y);
			queue.push(childId);
		}
	}

	// 3. Any unreached visible nodes get random positions near the center.
	//    Invisible nodes (Community/Process) are deliberately NOT added to
	//    the graph — their presence would stretch Sigma's bounding box and
	//    shrink the visible cluster. Community coloring is already computed
	//    from edges above, so the nodes themselves aren't needed.
	bulkNodes.forEach((node) => {
		const id = nodeKey(node);
		if (graph.hasNode(id)) return;
		const label = asNodeLabel(node.label);
		if (INVISIBLE_LABELS.has(label)) return;
		const x = (Math.random() - 0.5) * structuralSpread * 0.3;
		const y = (Math.random() - 0.5) * structuralSpread * 0.3;
		nodePositions.set(id, { x, y });
		addNode(id, x, y);
	});

	// ---------------------------------------------------------------------
	// Edges
	// ---------------------------------------------------------------------

	const edgeBaseSize = nodeCount > 20000 ? 0.4 : nodeCount > 5000 ? 0.6 : 1.0;

	// Per-edge-type size multipliers. Structural edges are thinner so they
	// don't dominate the call graph; CALLS/EXTENDS/IMPLEMENTS are thicker
	// because they carry more meaning at a glance.
	const EDGE_SIZE_MULTIPLIER: Record<string, number> = {
		CONTAINS: 0.4,
		DEFINES: 0.5,
		IMPORTS: 0.6,
		CALLS: 0.8,
		EXTENDS: 1.0,
		IMPLEMENTS: 0.9,
		OVERRIDES: 0.8,
		HAS_METHOD: 0.5,
		HAS_PROPERTY: 0.4,
		HAS_PARAMETER: 0.3,
		ACCESSES: 0.4,
		DECORATES: 0.4,
		MEMBER_OF: 0.3,
		STEP_IN_PROCESS: 0.7,
		TESTED_BY: 0.6,
		HANDLES_TOOL: 0.7,
		QUERIES: 0.6,
	};

	for (const rel of bulkEdges) {
		const src = ekSrc(rel);
		const tgt = ekTgt(rel);
		if (!graph.hasNode(src) || !graph.hasNode(tgt)) continue;
		// graphology's simple-graph mode rejects parallel edges; ignore dupes.
		if (graph.hasEdge(src, tgt)) continue;
		const multiplier = EDGE_SIZE_MULTIPLIER[rel.edge_type] ?? 0.5;
		graph.addEdge(src, tgt, {
			size: edgeBaseSize * multiplier,
			color: "#3a3a4a",
			relationType: rel.edge_type,
			type: "arrow",
		});
	}

	return graph;
};

// ---------------------------------------------------------------------------
// Filter helpers — used by the sidebar toggles.
// ---------------------------------------------------------------------------

export const filterGraphByLabels = (
	graph: Graph<SigmaNodeAttributes, SigmaEdgeAttributes>,
	visibleLabels: NodeLabel[],
): void => {
	const visible = new Set(visibleLabels);
	graph.forEachNode((nodeId, attrs) => {
		if (INVISIBLE_LABELS.has(attrs.nodeType)) {
			graph.setNodeAttribute(nodeId, "hidden", true);
			return;
		}
		graph.setNodeAttribute(nodeId, "hidden", !visible.has(attrs.nodeType));
	});
};

/** Return all nodes reachable from `startNodeId` within `maxHops` steps. */
export const getNodesWithinHops = (
	graph: Graph<SigmaNodeAttributes, SigmaEdgeAttributes>,
	startNodeId: string,
	maxHops: number,
): Set<string> => {
	const visited = new Set<string>();
	const queue: { nodeId: string; depth: number }[] = [{ nodeId: startNodeId, depth: 0 }];
	while (queue.length > 0) {
		const { nodeId, depth } = queue.shift()!;
		if (visited.has(nodeId)) continue;
		visited.add(nodeId);
		if (depth < maxHops) {
			graph.forEachNeighbor(nodeId, (neighborId) => {
				if (!visited.has(neighborId)) {
					queue.push({ nodeId: neighborId, depth: depth + 1 });
				}
			});
		}
	}
	return visited;
};

export const filterGraphByDepth = (
	graph: Graph<SigmaNodeAttributes, SigmaEdgeAttributes>,
	selectedNodeId: string | null,
	maxHops: number | null,
	visibleLabels: NodeLabel[],
): void => {
	if (maxHops === null || selectedNodeId === null || !graph.hasNode(selectedNodeId)) {
		filterGraphByLabels(graph, visibleLabels);
		return;
	}
	const inRange = getNodesWithinHops(graph, selectedNodeId, maxHops);
	const visible = new Set(visibleLabels);
	graph.forEachNode((nodeId, attrs) => {
		if (INVISIBLE_LABELS.has(attrs.nodeType)) {
			graph.setNodeAttribute(nodeId, "hidden", true);
			return;
		}
		const labelOk = visible.has(attrs.nodeType);
		graph.setNodeAttribute(nodeId, "hidden", !labelOk || !inRange.has(nodeId));
	});
};

// ---------------------------------------------------------------------------
// Layout modes — reposition nodes without rebuilding the graph.
// ---------------------------------------------------------------------------

export type LayoutMode = "force" | "solar" | "cluster" | "tree";

// Ring assignments for Solar layout. Inner rings = structural, outer = symbols.
const SOLAR_RING: Record<string, number> = {
	Project: 0, Package: 0, Module: 0, Namespace: 0,
	Folder: 1,
	File: 2,
};
// Everything else goes to ring 3 (outermost).

/** Solar layout — 4 concentric rings by structural level. */
export const applySolarLayout = (
	graph: Graph<SigmaNodeAttributes, SigmaEdgeAttributes>,
): void => {
	// Bucket nodes into rings.
	const rings: string[][] = [[], [], [], []];
	graph.forEachNode((nodeId, attrs) => {
		if (attrs.hidden) return;
		const ring = SOLAR_RING[attrs.nodeType] ?? 3;
		rings[ring].push(nodeId);
	});

	const baseRadius = Math.sqrt(graph.order) * 8;
	const ringRadii = [baseRadius * 0.3, baseRadius * 0.6, baseRadius * 1.0, baseRadius * 1.6];

	for (let r = 0; r < rings.length; r++) {
		const nodes = rings[r];
		const radius = ringRadii[r];
		nodes.forEach((nodeId, i) => {
			const angle = (i / Math.max(nodes.length, 1)) * Math.PI * 2;
			// Small jitter to avoid perfect overlaps.
			const jitter = radius * 0.05 * (Math.random() - 0.5);
			graph.setNodeAttribute(nodeId, "x", (radius + jitter) * Math.cos(angle));
			graph.setNodeAttribute(nodeId, "y", (radius + jitter) * Math.sin(angle));
		});
	}
};

/** Cluster layout — nodes grouped by community in a grid of clusters. */
export const applyClusterLayout = (
	graph: Graph<SigmaNodeAttributes, SigmaEdgeAttributes>,
): void => {
	// Group visible nodes by community.
	const clusters = new Map<number, string[]>();
	const unclustered: string[] = [];

	graph.forEachNode((nodeId, attrs) => {
		if (attrs.hidden) return;
		if (attrs.community !== undefined && attrs.community !== null) {
			if (!clusters.has(attrs.community)) clusters.set(attrs.community, []);
			clusters.get(attrs.community)!.push(nodeId);
		} else {
			unclustered.push(nodeId);
		}
	});

	const clusterCount = clusters.size + (unclustered.length > 0 ? 1 : 0);
	const spread = Math.sqrt(graph.order) * 12;
	const goldenAngle = Math.PI * (3 - Math.sqrt(5));

	// Position each cluster in a spiral.
	let clusterIdx = 0;
	const positionCluster = (nodes: string[]) => {
		const angle = clusterIdx * goldenAngle;
		const radius = spread * Math.sqrt((clusterIdx + 1) / Math.max(clusterCount, 1));
		const cx = radius * Math.cos(angle);
		const cy = radius * Math.sin(angle);
		const clusterRadius = Math.sqrt(nodes.length) * 4;
		nodes.forEach((nodeId, i) => {
			const a = (i / Math.max(nodes.length, 1)) * Math.PI * 2;
			const r = clusterRadius * Math.sqrt((i + 1) / nodes.length);
			graph.setNodeAttribute(nodeId, "x", cx + r * Math.cos(a));
			graph.setNodeAttribute(nodeId, "y", cy + r * Math.sin(a));
		});
		clusterIdx++;
	};

	for (const nodes of clusters.values()) {
		positionCluster(nodes);
	}
	if (unclustered.length > 0) {
		positionCluster(unclustered);
	}
};

/** Tree layout — hierarchical top-down by CONTAINS/DEFINES edges. */
export const applyTreeLayout = (
	graph: Graph<SigmaNodeAttributes, SigmaEdgeAttributes>,
): void => {
	// Build parent → children from hierarchy edges.
	const children = new Map<string, string[]>();
	const hasParent = new Set<string>();

	graph.forEachEdge((_edge, attrs, source, target) => {
		const rel = attrs.relationType;
		if (rel === "CONTAINS" || rel === "DEFINES") {
			if (!children.has(source)) children.set(source, []);
			children.get(source)!.push(target);
			hasParent.add(target);
		}
	});

	// Find roots (nodes with no parent in the hierarchy).
	const roots: string[] = [];
	graph.forEachNode((nodeId, attrs) => {
		if (!attrs.hidden && !hasParent.has(nodeId)) {
			roots.push(nodeId);
		}
	});

	const levelSpacing = 80;
	const leafSpacing = 8;
	let leafIndex = 0;

	// BFS to assign depth and horizontal position.
	const visited = new Set<string>();
	const queue: { id: string; depth: number }[] = roots.map((id) => ({ id, depth: 0 }));

	while (queue.length > 0) {
		const { id, depth } = queue.shift()!;
		if (visited.has(id)) continue;
		visited.add(id);

		const kids = children.get(id) ?? [];
		const hasKids = kids.length > 0;

		// Leaf nodes get sequential x positions; branch nodes will be
		// centered over their children after the traversal.
		if (!hasKids) {
			graph.setNodeAttribute(id, "x", leafIndex * leafSpacing);
			graph.setNodeAttribute(id, "y", depth * levelSpacing);
			leafIndex++;
		} else {
			// Placeholder — will center after children are placed.
			graph.setNodeAttribute(id, "y", depth * levelSpacing);
			for (const kid of kids) {
				if (!visited.has(kid)) {
					queue.push({ id: kid, depth: depth + 1 });
				}
			}
		}
	}

	// Second pass: center branch nodes over their children (bottom-up).
	const centerParents = (nodeId: string): number => {
		const kids = (children.get(nodeId) ?? []).filter((k) => visited.has(k));
		if (kids.length === 0) {
			return graph.getNodeAttribute(nodeId, "x") as number;
		}
		const childXs = kids.map((k) => centerParents(k));
		const cx = childXs.reduce((a, b) => a + b, 0) / childXs.length;
		graph.setNodeAttribute(nodeId, "x", cx);
		return cx;
	};

	for (const root of roots) {
		centerParents(root);
	}

	// Place any orphan nodes not reached by the tree walk.
	graph.forEachNode((nodeId) => {
		if (!visited.has(nodeId)) {
			graph.setNodeAttribute(nodeId, "x", leafIndex * leafSpacing);
			graph.setNodeAttribute(nodeId, "y", 0);
			leafIndex++;
		}
	});
};
