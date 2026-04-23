// Bounded exponential-backoff retry for network-level fetch failures.
//
// In Tauri desktop builds the frontend can race the sidecar backend:
// the HTTP listener on 127.0.0.1:19898 may bind before (or briefly
// after) lazy subsystems like LadybugDB, FTS indexing, and ONNX are
// ready. During that window fetch() throws a TypeError
// (ERR_CONNECTION_REFUSED / ERR_CONNECTION_RESET) instead of returning
// a Response. This helper retries ONLY those connect-level failures.
// 4xx/5xx responses are returned as-is — they represent real
// application errors and must surface to the caller immediately.

// Budget sized to cover the sidecar's observed 10-15s warmup window:
// sum of delays across ATTEMPTS is ~16s worst-case.
const ATTEMPTS = 8;
const BASE_MS = 300;
const CAP_MS = 3000;

export async function fetchWithRetry(
	url: string,
	init?: RequestInit,
): Promise<Response> {
	let lastErr: unknown;
	for (let attempt = 0; attempt < ATTEMPTS; attempt++) {
		try {
			return await fetch(url, init);
		} catch (err) {
			if (init?.signal?.aborted) throw err;
			lastErr = err;
			if (attempt === ATTEMPTS - 1) break;
			const delay = Math.min(BASE_MS * 2 ** attempt, CAP_MS);
			await new Promise((r) => setTimeout(r, delay));
		}
	}
	throw lastErr;
}
