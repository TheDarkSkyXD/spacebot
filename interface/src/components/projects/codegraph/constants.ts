// Node and edge visual constants for the code graph canvas.
// Ported from reference/GitNexus/gitnexus-web/src/lib/constants.ts.
//
// Taxonomy:
//   - 11 filter-facing node types and 6 edge types — matches GitNexus exactly.
//   - Backend emits ~40 raw labels (see schema::DISPLAY_NODE_LABELS).
//     CANONICAL_LABEL MUST map every raw label onto one of the 11 filter
//     types so "uncheck every filter" really hides every node. Rendering
//     still uses the raw label so Project vs. Folder stays visually distinct.

export type NodeLabel =
	| "Project"
	| "Package"
	| "Module"
	| "Namespace"
	| "Folder"
	| "File"
	| "Class"
	| "Interface"
	| "Enum"
	| "Type"
	| "Function"
	| "Method"
	| "Variable"
	| "Decorator"
	| "Import"
	| "Struct"
	| "Trait"
	| "Impl"
	| "TypeAlias"
	| "Const"
	| "MacroDef"
	| "Record"
	| "Template"
	| "Test"
	| "Route"
	| "Section"
	| "Tool"
	| "Community"
	| "Process"
	// Additional raw labels emitted by the backend pipeline.
	// Each one MUST have a CANONICAL_LABEL entry below.
	| "Property"
	| "Constructor"
	| "Typedef"
	| "UnionType"
	| "Static"
	| "Delegate"
	| "CodeElement"
	| "Middleware"
	| "SqlQuery"
	| "CicsCall"
	| "DliCall"
	| "JclJob"
	| "JclStep";

export type EdgeType =
	| "CONTAINS"
	| "DEFINES"
	| "IMPORTS"
	| "CALLS"
	| "EXTENDS"
	| "IMPLEMENTS";

// Maps every raw backend label onto one of the 11 filter types.
// A node with label L is treated as (CANONICAL_LABEL[L] ?? L) when the
// filter panel toggles it.
//
// Every entry in DISPLAY_NODE_LABELS (src/codegraph/schema.rs) that isn't
// itself one of the 11 filterable targets needs a mapping here, otherwise
// that label leaks through the filter and nodes become uncontrollable
// from the UI. Community / Process are deliberately absent — they're
// invisible regardless of filter state.
export const CANONICAL_LABEL: Partial<Record<NodeLabel, NodeLabel>> = {
	// Structural containers → Folder (hierarchy, not a symbol).
	Project: "Folder",
	Package: "Folder",
	Module: "Folder",
	Namespace: "Folder",
	// Section = a markdown heading (sub-unit of a File). Canonicalizing to
	// File means the File toggle hides markdown content, and the Folder
	// toggle leaves .md / .mdx files alone.
	Section: "File",
	// Class-ish
	Struct: "Class",
	Record: "Class",
	// Interface-ish
	Trait: "Interface",
	// Method-ish
	Impl: "Method",
	Constructor: "Method",
	// Type-ish
	TypeAlias: "Type",
	Template: "Type",
	Typedef: "Type",
	UnionType: "Type",
	Delegate: "Type",
	// Function-ish (executable / entry-point / handler code).
	Test: "Function",
	Route: "Function",
	Tool: "Function",
	Middleware: "Function",
	SqlQuery: "Function",
	CicsCall: "Function",
	DliCall: "Function",
	JclStep: "Function",
	CodeElement: "Function",
	// Variable-ish
	Const: "Variable",
	Property: "Variable",
	Static: "Variable",
	// Decorator-ish
	MacroDef: "Decorator",
	// File-ish (JCL job = a runnable script file, closest to File).
	JclJob: "File",
};

export const toCanonicalLabel = (label: NodeLabel): NodeLabel =>
	CANONICAL_LABEL[label] ?? label;

// ---------------------------------------------------------------------------
// Colors — similar tokens reused across related labels.
// ---------------------------------------------------------------------------

export const NODE_COLORS: Record<NodeLabel, string> = {
	Project: "#a855f7",
	Package: "#8b5cf6",
	Module: "#7c3aed",
	Namespace: "#7c3aed",
	Folder: "#6366f1",
	File: "#3b82f6",
	Class: "#f59e0b",
	Interface: "#ec4899",
	Enum: "#f97316",
	Type: "#a78bfa",
	Function: "#10b981",
	Method: "#14b8a6",
	Variable: "#64748b",
	Decorator: "#eab308",
	Import: "#475569",
	Struct: "#f59e0b",
	Trait: "#ec4899",
	Impl: "#14b8a6",
	TypeAlias: "#a78bfa",
	Const: "#64748b",
	MacroDef: "#eab308",
	Record: "#f59e0b",
	Template: "#a78bfa",
	Test: "#84cc16",
	Route: "#f43f5e",
	Tool: "#a855f7",
	Section: "#60a5fa",
	Community: "#818cf8",
	Process: "#f43f5e",
	// Colors for additional raw labels — borrow the color of the
	// canonical target so the legend stays visually coherent.
	Property: "#64748b",     // Variable
	Constructor: "#14b8a6",  // Method
	Typedef: "#a78bfa",      // Type
	UnionType: "#a78bfa",    // Type
	Static: "#64748b",       // Variable
	Delegate: "#a78bfa",     // Type
	CodeElement: "#10b981",  // Function
	Middleware: "#10b981",   // Function
	SqlQuery: "#10b981",     // Function
	CicsCall: "#10b981",     // Function
	DliCall: "#10b981",      // Function
	JclJob: "#3b82f6",       // File
	JclStep: "#10b981",      // Function
};

// ---------------------------------------------------------------------------
// Sizes — larger = more visual weight in the force-directed layout.
// Community and Process are 0 because they're metadata, not visible
// graph nodes. The adapter hides them from the canvas entirely.
// ---------------------------------------------------------------------------

export const NODE_SIZES: Record<NodeLabel, number> = {
	Project: 20,
	Package: 16,
	Module: 13,
	Namespace: 13,
	Folder: 10,
	File: 6,
	Class: 8,
	Interface: 7,
	Enum: 5,
	Type: 3,
	Function: 4,
	Method: 3,
	Variable: 2,
	Decorator: 2,
	Import: 1.5,
	Struct: 8,
	Trait: 7,
	Impl: 3,
	TypeAlias: 3,
	Const: 2,
	MacroDef: 2,
	Record: 8,
	Template: 3,
	Test: 4,
	Route: 5,
	Tool: 5,
	Section: 6,
	Community: 0,
	Process: 0,
	// Sizes for additional raw labels — match the canonical target.
	Property: 2,
	Constructor: 3,
	Typedef: 3,
	UnionType: 3,
	Static: 2,
	Delegate: 3,
	CodeElement: 4,
	Middleware: 4,
	SqlQuery: 4,
	CicsCall: 4,
	DliCall: 4,
	JclJob: 6,
	JclStep: 4,
};

// Community color palette for cluster-based coloring of symbol nodes.
export const COMMUNITY_COLORS = [
	"#ef4444",
	"#f97316",
	"#eab308",
	"#22c55e",
	"#06b6d4",
	"#3b82f6",
	"#8b5cf6",
	"#d946ef",
	"#ec4899",
	"#f43f5e",
	"#14b8a6",
	"#84cc16",
];

export const getCommunityColor = (communityIndex: number): string => {
	return COMMUNITY_COLORS[communityIndex % COMMUNITY_COLORS.length];
};

// ---------------------------------------------------------------------------
// Filter taxonomy — 11 filterable node types, 6 edges. Every raw backend
// label must canonicalize (via CANONICAL_LABEL) to one of FILTERABLE_LABELS,
// and DEFAULT_VISIBLE_LABELS must be a strict subset of FILTERABLE_LABELS.
// Otherwise "uncheck every toggle" leaves orphan labels visible with no UI
// to hide them. Variable / Decorator / Import are togglable but off by default.
// ---------------------------------------------------------------------------

export const DEFAULT_VISIBLE_LABELS: NodeLabel[] = [
	"Folder",
	"File",
	"Class",
	"Function",
	"Method",
	"Interface",
	"Enum",
	"Type",
];

export const FILTERABLE_LABELS: NodeLabel[] = [
	"Folder",
	"File",
	"Class",
	"Interface",
	"Enum",
	"Type",
	"Function",
	"Method",
	"Variable",
	"Decorator",
	"Import",
];

// ---------------------------------------------------------------------------
// Edge types and their display colors.
// ---------------------------------------------------------------------------

export const ALL_EDGE_TYPES: EdgeType[] = [
	"CONTAINS",
	"DEFINES",
	"IMPORTS",
	"CALLS",
	"EXTENDS",
	"IMPLEMENTS",
];

export const DEFAULT_VISIBLE_EDGES: EdgeType[] = [
	"CONTAINS",
	"DEFINES",
	"IMPORTS",
	"CALLS",
	"EXTENDS",
	"IMPLEMENTS",
];

export const EDGE_INFO: Record<EdgeType, { color: string; label: string }> = {
	CONTAINS: { color: "#2d5a3d", label: "Contains" },
	DEFINES: { color: "#0e7490", label: "Defines" },
	IMPORTS: { color: "#1d4ed8", label: "Imports" },
	CALLS: { color: "#7c3aed", label: "Calls" },
	EXTENDS: { color: "#c2410c", label: "Extends" },
	IMPLEMENTS: { color: "#be185d", label: "Implements" },
};
