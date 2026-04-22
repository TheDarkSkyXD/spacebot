import type { CodeGraphLanguageCount } from "@/api/client";
import { languageColor } from "@/lib/languageColors";

interface LanguageBreakdownProps {
	breakdown: CodeGraphLanguageCount[] | undefined;
	/** Max rows surfaced before the remainder collapses into "Other". */
	topN?: number;
}

interface Slice {
	name: string;
	count: number;
	percent: number;
	color: string;
}

const OTHER_COLOR = "#8b8b8b";

function buildSlices(
	breakdown: CodeGraphLanguageCount[],
	topN: number,
): Slice[] | null {
	const total = breakdown.reduce((sum, entry) => sum + entry.count, 0);
	if (total === 0) return null;

	const sorted = [...breakdown].sort((a, b) => b.count - a.count);
	const top = sorted.slice(0, topN);
	const rest = sorted.slice(topN);
	const restCount = rest.reduce((sum, entry) => sum + entry.count, 0);

	const slices: Slice[] = top.map((entry) => ({
		name: entry.name,
		count: entry.count,
		percent: (entry.count / total) * 100,
		color: languageColor(entry.name),
	}));
	if (restCount > 0) {
		slices.push({
			name: "Other",
			count: restCount,
			percent: (restCount / total) * 100,
			color: OTHER_COLOR,
		});
	}
	return slices;
}

function formatPercent(pct: number): string {
	return `${pct.toFixed(1)}%`;
}

export function LanguageBreakdown({ breakdown, topN = 4 }: LanguageBreakdownProps) {
	if (!breakdown || breakdown.length === 0) return null;
	const slices = buildSlices(breakdown, topN);
	if (!slices) return null;

	return (
		<div className="space-y-2">
			<p className="text-sm font-semibold text-ink">Languages</p>
			<div className="flex h-2 gap-0.5 overflow-hidden rounded-full bg-app-line">
				{slices.map((slice) => (
					<div
						key={slice.name}
						className="h-full first:rounded-l-full last:rounded-r-full"
						style={{
							width: `${slice.percent}%`,
							backgroundColor: slice.color,
						}}
						title={`${slice.name} — ${formatPercent(slice.percent)}`}
					/>
				))}
			</div>
			<div className="flex flex-wrap gap-x-4 gap-y-1 text-[11px]">
				{slices.map((slice) => (
					<div key={slice.name} className="flex items-center gap-1.5">
						<span
							className="h-2 w-2 shrink-0 rounded-full"
							style={{ backgroundColor: slice.color }}
						/>
						<span className="text-ink">{slice.name}</span>
						<span className="text-ink-faint">{formatPercent(slice.percent)}</span>
					</div>
				))}
			</div>
		</div>
	);
}
