// Stream newline-delimited JSON records from an HTTP response.
//
// Mirrors GitNexus's `parseNdjsonGraphResponse` (gitnexus-web backend
// client): read bytes, keep the trailing partial line in a buffer, yield
// complete records one at a time. Used by the code-graph streaming
// endpoint so massive graphs can be delivered without materializing a
// giant JSON body on the server.
export async function* fetchNdjson<T>(
	url: string,
	init?: RequestInit,
): AsyncGenerator<T, void, unknown> {
	const res = await fetch(url, init);
	if (!res.ok) {
		throw new Error(`API error: ${res.status}`);
	}
	if (!res.body) {
		throw new Error("NDJSON response has no body");
	}

	const reader = res.body.getReader();
	const decoder = new TextDecoder();
	let buffer = "";

	try {
		while (true) {
			const { done, value } = await reader.read();
			if (done) break;
			buffer += decoder.decode(value, { stream: true });
			const lines = buffer.split("\n");
			buffer = lines.pop() ?? "";
			for (const line of lines) {
				if (!line) continue;
				yield JSON.parse(line) as T;
			}
		}
		// Flush any trailing bytes and emit the final line if present.
		buffer += decoder.decode();
		const trimmed = buffer.trim();
		if (trimmed) {
			yield JSON.parse(trimmed) as T;
		}
	} finally {
		reader.releaseLock();
	}
}
