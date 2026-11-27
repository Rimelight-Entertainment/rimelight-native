/**
 * The base for stored file links on this instance: the owner's own Worker, which serves objects through
 * `GET /api/storage/object/<key>` after the Access check. `requestUrl` is any URL on the instance.
 * No shared CDN: the personal app never depends on another site's domain.
 */
export function filesBase(requestUrl: string): string {
  return `${new URL(requestUrl).origin}/api/storage/object`;
}

/** Keeps the folder path shape (no leading, trailing or empty segments, no `.` or `..`). */
export function cleanFolder(folder: string): string {
  return folder
    .split("/")
    .map((s) => s.trim())
    .filter((s) => s && s !== "." && s !== "..")
    .join("/");
}

export type MovePlan =
  | { ok: true; key: string; filename: string; url: string; changed: boolean }
  | { ok: false; error: string };

/**
 * Plans `PATCH /api/storage/files`: rename the basename and/or move to another folder.
 * A basename without an extension keeps the current one. Missing fields keep their current
 * value.
 */
export function planMove(input: {
  currentKey: string;
  currentFilename: string;
  basename?: unknown;
  folder?: unknown;
  publicBase: string;
}): MovePlan {
  const slash = input.currentKey.lastIndexOf("/");
  const currentFolder = slash >= 0 ? input.currentKey.slice(0, slash) : "";
  const folder = typeof input.folder === "string" ? cleanFolder(input.folder) : currentFolder;

  let filename = input.currentFilename;
  if (typeof input.basename === "string") {
    const trimmed = input.basename.trim();
    if (!trimmed) return { ok: false, error: "Missing basename" };
    let name = trimmed.replace(/[^a-zA-Z0-9._-]/g, "-");
    const dot = input.currentFilename.lastIndexOf(".");
    const ext = dot > 0 ? input.currentFilename.slice(dot) : "";
    if (ext && !name.includes(".")) name += ext;
    filename = name;
  }
  if (!filename || filename === "." || filename === "..") {
    return { ok: false, error: "Invalid name" };
  }

  const key = folder ? `${folder}/${filename}` : filename;
  return {
    ok: true,
    key,
    filename,
    url: `${input.publicBase}/${key}`,
    changed: key !== input.currentKey,
  };
}
