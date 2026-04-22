// Shared color picker popover content. Used by the filter sidebar
// (CodeGraphSidebar) and the mermaid view (file cards + group containers).

import { clsx } from "clsx";
import * as Popover from "@radix-ui/react-popover";

export const COLOR_PRESETS = [
	"#ef4444", "#f97316", "#eab308", "#22c55e",
	"#06b6d4", "#3b82f6", "#8b5cf6", "#d946ef",
	"#ec4899", "#f43f5e", "#14b8a6", "#84cc16",
	"#64748b", "#e2e8f0", "#a78bfa", "#fb923c",
];

interface Props {
	currentColor: string;
	defaultColor: string;
	onSelect: (color: string) => void;
	onReset: () => void;
}

export function NodeColorPicker({ currentColor, defaultColor, onSelect, onReset }: Props) {
	const isCustom = currentColor.toLowerCase() !== defaultColor.toLowerCase();
	return (
		<div className="flex flex-col gap-2">
			<div className="grid grid-cols-4 gap-1.5">
				{COLOR_PRESETS.map((c) => (
					<Popover.Close asChild key={c}>
						<button
							onClick={() => onSelect(c)}
							className={clsx(
								"h-6 w-6 rounded-full border-2 transition-transform hover:scale-110",
								c.toLowerCase() === currentColor.toLowerCase()
									? "border-white scale-110"
									: "border-transparent",
							)}
							style={{ backgroundColor: c }}
							title={c}
						/>
					</Popover.Close>
				))}
			</div>
			<div className="flex items-center gap-2 border-t border-app-line pt-2">
				<label className="flex cursor-pointer items-center gap-1.5 text-[10px] text-ink-faint">
					Custom
					<input
						type="color"
						defaultValue={currentColor}
						ref={(el) => {
							if (!el) return;
							// Native change event fires once on picker close instead
							// of React onChange which fires on every drag.
							el.onchange = () => onSelect(el.value);
						}}
						className="h-5 w-5 cursor-pointer rounded border-0 bg-transparent p-0"
					/>
				</label>
				{isCustom && (
					<Popover.Close asChild>
						<button
							onClick={onReset}
							className="ml-auto rounded px-2 py-0.5 text-[10px] text-ink-faint transition-colors hover:bg-app-hover hover:text-ink"
						>
							Reset
						</button>
					</Popover.Close>
				)}
			</div>
		</div>
	);
}
