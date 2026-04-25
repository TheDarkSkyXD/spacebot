// ---------------------------------------------------------------------------
// Code Graph API: types + methods. Imported from ./client to keep the
// codegraph subsystem self-contained (it adds ~370 lines that we don't
// want to merge inline with upstream's hand-written client.ts).
// ---------------------------------------------------------------------------

import { fetchJson, getApiBase } from "./client";
import { fetchNdjson } from "./ndjson";

// ---------------------------------------------------------------------------
// Code Graph types
// ---------------------------------------------------------------------------

export type CodeGraphIndexStatus = "pending" | "indexing" | "indexed" | "stale" | "error";

export interface CodeGraphProject {
	project_id: string;
	name: string;
	root_path: string;
	status: CodeGraphIndexStatus;
	progress?: {
		phase: string;
		phase_progress: number;
		message: string;
		stats: CodeGraphPipelineStats;
	};
	error_message?: string;
	last_index_stats?: CodeGraphPipelineStats;
	last_indexed_at?: string;
	primary_language?: string;
	language_breakdown?: CodeGraphLanguageCount[];
	schema_version: number;
	created_at: string;
	updated_at: string;
}

export interface CodeGraphLanguageCount {
	name: string;
	count: number;
}

export interface CodeGraphPipelineStats {
	files_found: number;
	files_parsed: number;
	files_skipped: number;
	nodes_created: number;
	edges_created: number;
	communities_detected: number;
	processes_traced: number;
	errors: number;
}

export interface CodeGraphProjectListResponse {
	projects: CodeGraphProject[];
}

export interface CodeGraphProjectDetailResponse {
	project: CodeGraphProject;
}

export interface CodeGraphCommunity {
	id: string;
	name: string;
	description?: string;
	node_count: number;
	file_count: number;
	function_count: number;
	key_symbols: string[];
}

export interface CodeGraphCommunitiesResponse {
	communities: CodeGraphCommunity[];
	total: number;
}

export interface CodeGraphProcess {
	id: string;
	entry_function: string;
	source_file: string;
	call_depth: number;
	community?: string;
	steps: string[];
}

export interface CodeGraphProcessesResponse {
	processes: CodeGraphProcess[];
	total: number;
}

export interface CodeGraphSearchResult {
	node_id: number;
	qualified_name: string;
	name: string;
	label: string;
	source_file?: string;
	line_start?: number;
	score: number;
	community?: string;
	snippet?: string;
}

export interface CodeGraphSearchResponse {
	results: CodeGraphSearchResult[];
	total: number;
}

export interface CodeGraphIndexLogEntry {
	run_id: string;
	status: CodeGraphIndexStatus;
	started_at: string;
	completed_at?: string;
	current_phase?: string;
	progress?: { phase: string; phase_progress: number; message: string };
	stats?: CodeGraphPipelineStats;
	error?: string;
}

export interface CodeGraphIndexLogResponse {
	entries: CodeGraphIndexLogEntry[];
}

export interface CodeGraphRemoveInfoResponse {
	node_count: number;
	edge_count: number;
}

export interface CodeGraphActionResponse {
	success: boolean;
	message: string;
}

// -- Node / Edge types for the graph explorer --

export type CodeGraphNodeLabel =
	| "project" | "package" | "module" | "folder" | "file"
	| "class" | "function" | "method" | "variable" | "parameter"
	| "interface" | "enum" | "decorator" | "import" | "type"
	| "struct" | "macro" | "trait" | "impl" | "namespace"
	| "type_alias" | "const" | "record" | "template"
	| "community" | "process" | "section" | "test" | "route";

export type CodeGraphEdgeType =
	| "CONTAINS" | "DEFINES" | "CALLS" | "IMPORTS" | "EXTENDS"
	| "IMPLEMENTS" | "OVERRIDES" | "HAS_METHOD" | "HAS_PROPERTY"
	| "ACCESSES" | "USES" | "HAS_PARAMETER" | "DECORATES"
	| "MEMBER_OF" | "STEP_IN_PROCESS" | "TESTED_BY"
	| "ENTRY_POINT_OF" | "HANDLES_ROUTE" | "FETCHES" | "QUERIES"
	| "HANDLES_TOOL";

export interface CodeGraphNodeSummary {
	id: number;
	qualified_name: string;
	name: string;
	label: string;
	source_file?: string;
	line_start?: number;
	line_end?: number;
	file_size?: number;
}

export interface CodeGraphNodeFull extends CodeGraphNodeSummary {
	source?: string;
	written_by?: string;
	properties: Record<string, unknown>;
}

export interface CodeGraphEdgeSummary {
	from_id: number;
	from_name: string;
	from_label: string;
	to_id: number;
	to_name: string;
	to_label: string;
	edge_type: string;
	confidence: number;
}

export interface CodeGraphLabelCount {
	label: string;
	count: number;
}

export interface CodeGraphTypeCount {
	edge_type: string;
	count: number;
}

export interface CodeGraphNodeListResponse {
	nodes: CodeGraphNodeSummary[];
	total: number;
	offset: number;
	limit: number;
}

export interface CodeGraphNodeDetailResponse {
	node: CodeGraphNodeFull;
}

export interface CodeGraphEdgeListResponse {
	edges: CodeGraphEdgeSummary[];
	total: number;
	offset: number;
	limit: number;
}

export interface CodeGraphStatsResponse {
	total_nodes: number;
	total_edges: number;
	nodes_by_label: CodeGraphLabelCount[];
	edges_by_type: CodeGraphTypeCount[];
}

export interface CodeGraphBulkNodesResponse {
	nodes: CodeGraphNodeSummary[];
}

export interface CodeGraphBulkEdgeSummary {
	from_qname: string;
	from_label: string;
	to_qname: string;
	to_label: string;
	edge_type: string;
	confidence: number;
}

export interface CodeGraphBulkEdgesResponse {
	edges: CodeGraphBulkEdgeSummary[];
}

export interface FsReadFileResponse {
	path: string;
	content: string;
	start_line: number;
	total_lines: number;
	language: string;
}


export const codegraphApi = {
	codegraphProjects: (status?: CodeGraphIndexStatus) => {
		const params = status ? `?status=${encodeURIComponent(status)}` : "";
		return fetchJson<CodeGraphProjectListResponse>(`/codegraph/projects${params}`);
	},

	codegraphProject: (projectId: string) =>
		fetchJson<CodeGraphProjectDetailResponse>(`/codegraph/projects/${encodeURIComponent(projectId)}`),

	codegraphCreateProject: async (name: string, rootPath: string): Promise<CodeGraphProjectDetailResponse> => {
		const response = await fetch(`${getApiBase()}/codegraph/projects`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ name, root_path: rootPath }),
		});
		if (!response.ok) throw new Error(`API error: ${response.status}`);
		return response.json() as Promise<CodeGraphProjectDetailResponse>;
	},

	codegraphDeleteProject: async (projectId: string): Promise<CodeGraphActionResponse> => {
		const response = await fetch(
			`${getApiBase()}/codegraph/projects/${encodeURIComponent(projectId)}`,
			{ method: "DELETE" },
		);
		if (!response.ok) throw new Error(`API error: ${response.status}`);
		return response.json() as Promise<CodeGraphActionResponse>;
	},

	codegraphReindex: async (projectId: string): Promise<CodeGraphActionResponse> => {
		const response = await fetch(
			`${getApiBase()}/codegraph/projects/${encodeURIComponent(projectId)}/reindex`,
			{ method: "POST" },
		);
		if (!response.ok) throw new Error(`API error: ${response.status}`);
		return response.json() as Promise<CodeGraphActionResponse>;
	},

	codegraphCommunities: (projectId: string) =>
		fetchJson<CodeGraphCommunitiesResponse>(`/codegraph/projects/${encodeURIComponent(projectId)}/graph/communities`),

	codegraphProcesses: (projectId: string) =>
		fetchJson<CodeGraphProcessesResponse>(`/codegraph/projects/${encodeURIComponent(projectId)}/graph/processes`),

	codegraphSearch: (projectId: string, query: string, limit = 20) =>
		fetchJson<CodeGraphSearchResponse>(
			`/codegraph/projects/${encodeURIComponent(projectId)}/graph/search?q=${encodeURIComponent(query)}&limit=${limit}`,
		),

	codegraphIndexLog: (projectId: string) =>
		fetchJson<CodeGraphIndexLogResponse>(`/codegraph/projects/${encodeURIComponent(projectId)}/graph/index-log`),

	codegraphRemoveInfo: (projectId: string) =>
		fetchJson<CodeGraphRemoveInfoResponse>(`/codegraph/projects/${encodeURIComponent(projectId)}/remove-info`),

	codegraphNodes: (projectId: string, params?: { label?: string; offset?: number; limit?: number }) => {
		const search = new URLSearchParams();
		if (params?.label) search.set("label", params.label);
		if (params?.offset != null) search.set("offset", String(params.offset));
		if (params?.limit != null) search.set("limit", String(params.limit));
		const qs = search.toString();
		return fetchJson<CodeGraphNodeListResponse>(
			`/codegraph/projects/${encodeURIComponent(projectId)}/graph/nodes${qs ? `?${qs}` : ""}`
		);
	},

	codegraphNode: (projectId: string, nodeId: number, label?: string) => {
		const qs = label ? `?label=${encodeURIComponent(label)}` : "";
		return fetchJson<CodeGraphNodeDetailResponse>(
			`/codegraph/projects/${encodeURIComponent(projectId)}/graph/nodes/${nodeId}${qs}`
		);
	},

	codegraphNodeEdges: (projectId: string, nodeId: number, params?: { direction?: string; edge_type?: string; offset?: number; limit?: number }) => {
		const search = new URLSearchParams();
		if (params?.direction) search.set("direction", params.direction);
		if (params?.edge_type) search.set("edge_type", params.edge_type);
		if (params?.offset != null) search.set("offset", String(params.offset));
		if (params?.limit != null) search.set("limit", String(params.limit));
		const qs = search.toString();
		return fetchJson<CodeGraphEdgeListResponse>(
			`/codegraph/projects/${encodeURIComponent(projectId)}/graph/nodes/${nodeId}/edges${qs ? `?${qs}` : ""}`
		);
	},

	codegraphStats: (projectId: string) =>
		fetchJson<CodeGraphStatsResponse>(`/codegraph/projects/${encodeURIComponent(projectId)}/graph/stats`),

	codegraphBulkNodes: (projectId: string) =>
		fetchJson<CodeGraphBulkNodesResponse>(
			`/codegraph/projects/${encodeURIComponent(projectId)}/graph/bulk-nodes`,
		),

	codegraphBulkEdges: (projectId: string) =>
		fetchJson<CodeGraphBulkEdgesResponse>(
			`/codegraph/projects/${encodeURIComponent(projectId)}/graph/bulk-edges`,
		),

	// Stream the full graph as NDJSON. Server yields one record per line:
	//   {"type":"node","data":{...}} | {"type":"edge","data":{...}} | {"type":"error","error":"..."}
	// Accumulates into the same {nodes, edges} shape the two paged endpoints
	// used to return, so callers keep their current contract. Matches
	// GitNexus's `/api/graph?stream=true` client pattern.
	codegraphGraphStream: async (
		projectId: string,
		signal?: AbortSignal,
		onProgress?: (p: { phase: "nodes" | "edges"; nodesLoaded: number; edgesLoaded: number }) => void,
	): Promise<{ nodes: CodeGraphNodeSummary[]; edges: CodeGraphBulkEdgeSummary[] }> => {
		type Record =
			| { type: "node"; data: CodeGraphNodeSummary }
			| { type: "edge"; data: CodeGraphBulkEdgeSummary }
			| { type: "error"; error: string };

		const nodes: CodeGraphNodeSummary[] = [];
		const edges: CodeGraphBulkEdgeSummary[] = [];
		const url = `${getApiBase()}/codegraph/projects/${encodeURIComponent(projectId)}/graph/stream`;

		// Throttle progress callbacks to ~every 100 records so setState
		// doesn't thrash React on large graphs.
		let i = 0;
		let phase: "nodes" | "edges" = "nodes";
		for await (const record of fetchNdjson<Record>(url, { signal })) {
			if (record.type === "node") {
				nodes.push(record.data);
				phase = "nodes";
			} else if (record.type === "edge") {
				edges.push(record.data);
				phase = "edges";
			} else if (record.type === "error") {
				throw new Error(record.error);
			}
			i++;
			if (onProgress && i % 100 === 0) {
				onProgress({ phase, nodesLoaded: nodes.length, edgesLoaded: edges.length });
			}
		}
		onProgress?.({ phase, nodesLoaded: nodes.length, edgesLoaded: edges.length });
		return { nodes, edges };
	},

};

