let apiKey = "";
/** Keep credentials only in this tab's memory. Reloading clears them. */
export function setApiKey(value: string): void {
  apiKey = value;
}
export async function response(
  path: string,
  body?: unknown,
): Promise<Response> {
  const response = await fetch(path, {
    ...(body === undefined
      ? {}
      : { method: "POST", body: JSON.stringify(body) }),
    headers: {
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
      ...(apiKey ? { Authorization: `Bearer ${apiKey}` } : {}),
    },
  });
  if (!response.ok)
    throw new Error(
      `Request failed (${response.status}): ${await response.text()}`,
    );
  return response;
}
export async function request<T>(path: string, body?: unknown): Promise<T> {
  return (await response(path, body)).json() as Promise<T>;
}
export async function download(path: string, filename: string): Promise<void> {
  const blob = await (await response(path)).blob();
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
