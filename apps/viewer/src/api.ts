export async function request<T>(path: string, body?: unknown): Promise<T> {
  const response = await fetch(
    path,
    body === undefined
      ? undefined
      : {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        },
  );
  if (!response.ok)
    throw new Error(
      `Request failed (${response.status}): ${await response.text()}`,
    );
  return response.json() as Promise<T>;
}
