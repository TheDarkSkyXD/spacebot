// Regenerate src/codegraph/langmap.rs from github-linguist's languages.yml
// filtered to entries present in ozh/github-colors. Emits two maps:
//   1. extension (lowercase, no dot) -> canonical language display name
//   2. basename (lowercase) -> canonical language display name
// When multiple languages share an extension, the popularity rank wins
// (a hardcoded static map can't do heuristic disambiguation anyway).
//
// Prereqs: bun + js-yaml (bun add -g js-yaml).
//
// Usage:
//   curl -sLo /tmp/languages.yml https://raw.githubusercontent.com/github-linguist/linguist/main/lib/linguist/languages.yml
//   curl -sL https://raw.githubusercontent.com/ozh/github-colors/master/colors.json | \
//     node -e "const d=require('/dev/stdin').read(); ..." > /tmp/language-colors.json
//   bun run scripts/gen_langmap.mjs > src/codegraph/langmap.rs
//
// Or update the two input paths below to point at your local copies.

import fs from "fs";
import yaml from "js-yaml";

const LANGUAGES_YML = process.env.LANGUAGES_YML || "/tmp/languages.yml";
const COLORS_JSON = process.env.COLORS_JSON || "interface/src/lib/githubLanguageColors.json";

const yml = yaml.load(fs.readFileSync(LANGUAGES_YML, "utf8"));
const colors = JSON.parse(fs.readFileSync(COLORS_JSON, "utf8"));

// Popularity rank — lower is preferred for tie-breaking. Extend when a
// new-but-common language loses to an obscure one with a shared extension.
const POPULARITY = [
	"JavaScript", "TypeScript", "TSX", "Python", "Java", "C", "C++", "C#",
	"Go", "Rust", "Ruby", "PHP", "Swift", "Kotlin", "Dart", "Scala",
	"Objective-C", "Perl", "Lua", "R", "Shell", "PowerShell", "Bash",
	"HTML", "CSS", "SCSS", "Sass", "Less", "Stylus", "Vue", "Svelte", "Astro",
	"JSON", "YAML", "TOML", "XML", "Markdown", "MDX", "reStructuredText",
	"Dockerfile", "Makefile", "CMake", "HCL", "GraphQL", "SQL", "Solidity",
	"Assembly", "WebAssembly", "Elixir", "Erlang", "Haskell", "OCaml", "F#",
	"Clojure", "Groovy", "Nim", "Crystal", "Zig", "V", "D", "Racket", "Elm",
	"Julia", "COBOL", "Vim Script", "Protocol Buffer",
];
const popRank = new Map(POPULARITY.map((n, i) => [n, i]));
function rank(name) {
	return popRank.has(name) ? popRank.get(name) : 1000 + name.charCodeAt(0);
}

// Manual overrides for ambiguous extensions / basenames. These win
// unconditionally — add an entry here when the rank heuristic picks an
// obscure language over a common one.
const EXT_OVERRIDES = {
	"yaml": "YAML",
	"yml": "YAML",
	"t": "Perl",
	"pl": "Perl",
	"m": "Objective-C",
	"h": "C",
	"s": "Assembly",
	"cls": "Apex",
	"v": "Verilog",
	"r": "R",
	"d": "D",
	"fs": "F#",
	// Fold TSX into TypeScript: linguist treats them as distinct for
	// coloring, but users expect `.tsx` to count as TypeScript on the
	// language breakdown (matches the mental model most repos use).
	"tsx": "TypeScript",
};
const NAME_OVERRIDES = {
	"dockerfile": "Dockerfile",
	"makefile": "Makefile",
	"rakefile": "Ruby",
	"gemfile": "Ruby",
};

const extCandidates = new Map();
const nameCandidates = new Map();

for (const [langName, info] of Object.entries(yml)) {
	if (!info) continue;
	if (!(langName in colors)) continue;
	const exts = info.extensions || [];
	exts.forEach((raw, i) => {
		const ext = raw.replace(/^\./, "").toLowerCase();
		if (!ext) return;
		if (!extCandidates.has(ext)) extCandidates.set(ext, []);
		extCandidates.get(ext).push({ lang: langName, primary: i === 0 });
	});
	const names = info.filenames || [];
	for (const raw of names) {
		const lower = raw.toLowerCase();
		if (!nameCandidates.has(lower)) nameCandidates.set(lower, []);
		nameCandidates.get(lower).push({ lang: langName, primary: true });
	}
}

function pick(candidates) {
	const sorted = [...candidates].sort((a, b) => {
		const ra = rank(a.lang);
		const rb = rank(b.lang);
		if (ra !== rb) return ra - rb;
		if (a.primary !== b.primary) return a.primary ? -1 : 1;
		return a.lang.localeCompare(b.lang);
	});
	return sorted[0].lang;
}

const extMap = new Map();
for (const [ext, cands] of extCandidates) {
	extMap.set(ext, (ext in EXT_OVERRIDES && EXT_OVERRIDES[ext] in colors)
		? EXT_OVERRIDES[ext]
		: pick(cands));
}
const nameMap = new Map();
for (const [name, cands] of nameCandidates) {
	nameMap.set(name, (name in NAME_OVERRIDES && NAME_OVERRIDES[name] in colors)
		? NAME_OVERRIDES[name]
		: pick(cands));
}

console.error(`extensions: ${extMap.size}`);
console.error(`filenames: ${nameMap.size}`);

function esc(s) { return s.replace(/\\/g, "\\\\").replace(/"/g, "\\\""); }

const extEntries = [...extMap.entries()].sort(([a], [b]) => a.localeCompare(b));
const nameEntries = [...nameMap.entries()].sort(([a], [b]) => a.localeCompare(b));

let out = "";
out += "// AUTO-GENERATED from github-linguist/languages.yml filtered to entries\n";
out += "// present in ozh/github-colors. Regenerate with scripts/gen_langmap.mjs.\n\n";
out += "use std::path::Path;\n\n";
out += "/// Look up an extension (without leading `.`, case-insensitive) in the\n";
out += "/// generated extension map. Uses binary search — the slice is sorted by key.\n";
out += "pub fn lookup_extension(ext: &str) -> Option<&'static str> {\n";
out += "    let lower = ext.to_ascii_lowercase();\n";
out += "    EXT_TO_LANG\n";
out += "        .binary_search_by_key(&lower.as_str(), |&(k, _)| k)\n";
out += "        .ok()\n";
out += "        .map(|i| EXT_TO_LANG[i].1)\n";
out += "}\n\n";
out += "/// Look up a basename (full filename, case-insensitive) in the generated\n";
out += "/// filename map. Covers extensionless files like `Dockerfile`, `Makefile`,\n";
out += "/// and lock/config files linguist recognizes by name.\n";
out += "pub fn lookup_basename(name: &str) -> Option<&'static str> {\n";
out += "    let lower = name.to_ascii_lowercase();\n";
out += "    NAME_TO_LANG\n";
out += "        .binary_search_by_key(&lower.as_str(), |&(k, _)| k)\n";
out += "        .ok()\n";
out += "        .map(|i| NAME_TO_LANG[i].1)\n";
out += "}\n\n";
out += "/// Resolve a file path to its language, checking basename first (so\n";
out += "/// `Dockerfile` / `Makefile` hit) and falling back to the extension.\n";
out += "pub fn language_for_path(path: &Path) -> Option<&'static str> {\n";
out += "    if let Some(name) = path.file_name().and_then(|s| s.to_str())\n";
out += "        && let Some(lang) = lookup_basename(name)\n";
out += "    {\n";
out += "        return Some(lang);\n";
out += "    }\n";
out += "    path.extension()\n";
out += "        .and_then(|s| s.to_str())\n";
out += "        .and_then(lookup_extension)\n";
out += "}\n\n";
out += "pub const EXT_TO_LANG: &[(&str, &str)] = &[\n";
for (const [ext, lang] of extEntries) {
	out += `    ("${esc(ext)}", "${esc(lang)}"),\n`;
}
out += "];\n\n";
out += "pub const NAME_TO_LANG: &[(&str, &str)] = &[\n";
for (const [name, lang] of nameEntries) {
	out += `    ("${esc(name)}", "${esc(lang)}"),\n`;
}
out += "];\n";

process.stdout.write(out);
