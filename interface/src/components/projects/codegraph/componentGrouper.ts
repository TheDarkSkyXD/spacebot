// Union-Find (disjoint-set) grouping of files by their cross-file
// relationships. Two files are in the same group iff there's a path of
// IMPORTS/CALLS/EXTENDS/IMPLEMENTS edges between them (after lifting
// symbol-level edges to their parent files).
//
// Output groups have a stable ID (hash of sorted member qnames) so
// user-edited title/color overrides can survive reindexes as long as the
// cluster shape doesn't drift drastically.

import type { CodeGraphBulkEdgeSummary } from "@/api/client";
import type { BulkNode } from "./types";

const FILE_EDGE_TYPES = new Set(["IMPORTS", "CALLS", "EXTENDS", "IMPLEMENTS"]);

export interface FileGroup {
	id: string; // stable hash of sorted member qnames
	defaultTitle: string;
	files: BulkNode[];
	isolated: boolean; // true for the bucket of files with no cross-file edges
}

function hashString(input: string): string {
	// djb2 — tiny, stable, good enough for cache keys.
	let h = 5381;
	for (let i = 0; i < input.length; i++) {
		h = ((h << 5) + h) ^ input.charCodeAt(i);
	}
	return (h >>> 0).toString(36);
}

function findRoot(parent: Map<string, string>, x: string): string {
	let r = x;
	while (parent.get(r)! !== r) r = parent.get(r)!;
	// Path compression.
	let cur = x;
	while (parent.get(cur)! !== r) {
		const next = parent.get(cur)!;
		parent.set(cur, r);
		cur = next;
	}
	return r;
}

function union(parent: Map<string, string>, rank: Map<string, number>, a: string, b: string) {
	const ra = findRoot(parent, a);
	const rb = findRoot(parent, b);
	if (ra === rb) return;
	const rankA = rank.get(ra) ?? 0;
	const rankB = rank.get(rb) ?? 0;
	if (rankA < rankB) parent.set(ra, rb);
	else if (rankA > rankB) parent.set(rb, ra);
	else { parent.set(rb, ra); rank.set(ra, rankA + 1); }
}

export function groupFilesByRelationships(
	allNodes: BulkNode[],
	allEdges: CodeGraphBulkEdgeSummary[],
): FileGroup[] {
	const files = allNodes.filter((n) => n.label === "File");
	if (files.length === 0) return [];

	const fileByQname = new Map<string, BulkNode>();
	const fileByPath = new Map<string, BulkNode>();
	for (const f of files) {
		fileByQname.set(f.qualified_name, f);
		if (f.source_file) fileByPath.set(f.source_file, f);
	}

	// Map every node qname to its parent file qname so symbol edges can be
	// promoted to file-to-file unions.
	const qnameToFileQname = new Map<string, string>();
	for (const node of allNodes) {
		if (!node.source_file) continue;
		const parent = fileByPath.get(node.source_file);
		if (parent) qnameToFileQname.set(node.qualified_name, parent.qualified_name);
	}
	for (const f of files) qnameToFileQname.set(f.qualified_name, f.qualified_name);

	// Initialize Union-Find with every file as its own component.
	const parent = new Map<string, string>();
	const rank = new Map<string, number>();
	for (const f of files) {
		parent.set(f.qualified_name, f.qualified_name);
		rank.set(f.qualified_name, 0);
	}

	// Track which files have at least one cross-file edge so we can split
	// true singletons into the "Isolated" bucket later.
	const hasEdge = new Set<string>();

	for (const edge of allEdges) {
		if (!FILE_EDGE_TYPES.has(edge.edge_type)) continue;
		const from = qnameToFileQname.get(edge.from_qname);
		const to = qnameToFileQname.get(edge.to_qname);
		if (!from || !to || from === to) continue;
		hasEdge.add(from);
		hasEdge.add(to);
		union(parent, rank, from, to);
	}

	// Collect components.
	const buckets = new Map<string, BulkNode[]>();
	for (const f of files) {
		const root = findRoot(parent, f.qualified_name);
		const arr = buckets.get(root) ?? [];
		arr.push(f);
		buckets.set(root, arr);
	}

	// Singletons that had no cross-file edge → into one "Isolated files"
	// group so we don't render hundreds of one-file containers.
	const isolated: BulkNode[] = [];
	const connectedGroups: BulkNode[][] = [];
	for (const [, members] of buckets) {
		if (members.length === 1 && !hasEdge.has(members[0].qualified_name)) {
			isolated.push(members[0]);
		} else {
			connectedGroups.push(members);
		}
	}

	// Sort connected groups by size desc so the biggest clusters render
	// first (top of the canvas).
	connectedGroups.sort((a, b) => b.length - a.length);

	const groups: FileGroup[] = connectedGroups.map((members) => {
		members.sort((a, b) => a.name.localeCompare(b.name));
		const sortedQnames = members.map((m) => m.qualified_name).sort();
		const id = "g_" + hashString(sortedQnames.join("\u0001"));
		const sample = members.slice(0, 3).map((m) => m.name).join(", ");
		const more = members.length > 3 ? `, +${members.length - 3}` : "";
		return {
			id,
			defaultTitle: `${sample}${more}`,
			files: members,
			isolated: false,
		};
	});

	if (isolated.length > 0) {
		isolated.sort((a, b) => a.name.localeCompare(b.name));
		groups.push({
			id: "g_isolated",
			defaultTitle: `Isolated files (${isolated.length})`,
			files: isolated,
			isolated: true,
		});
	}

	return groups;
}
