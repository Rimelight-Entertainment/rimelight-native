/**
 * Owner-scoped id handling shared by the write routes. The route adds `userId = owner` to every
 * UPDATE and DELETE, so a row that belongs to nobody else (or no one) matches nothing.
 */
export type OwnedTarget =
  | { ok: true; id: string; owner: string }
  | { ok: false; error: string };

export function planOwnedId(id: unknown, owner: string): OwnedTarget {
  if (typeof id !== "string" || !id.trim()) return { ok: false, error: "Missing id" };
  return { ok: true, id: id.trim(), owner };
}

/** Parses `?id=` or `?ids=a,b` into unique, non-empty ids. */
export function parseIds(id: string | undefined, ids: string | undefined): string[] {
  const raw = ids ? ids.split(",") : id ? [id] : [];
  return [...new Set(raw.map((s) => s.trim()).filter(Boolean))];
}
