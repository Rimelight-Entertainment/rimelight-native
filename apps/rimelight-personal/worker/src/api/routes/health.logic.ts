const isRecord = (v: unknown): v is Record<string, any> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

const toDate = (v: unknown): Date | null => {
  if (v === undefined || v === null || v === "") return null;
  const d = new Date(v as string | number | Date);
  return Number.isNaN(d.getTime()) ? null : d;
};

export type FastPlan =
  | { ok: true; startTime: Date; endTime: Date; originalEndTime: Date | null; isActive: boolean }
  | { ok: false; error: string };

/**
 * A new fast. The web sends `durationHours` (and no endTime): the planned end is start plus the
 * duration. An explicit `endTime` still wins.
 */
export function planFastPost(body: unknown, now: Date): FastPlan {
  const b = isRecord(body) ? body : {};
  const start = toDate(b.startTime) ?? now;
  const hours = Number(b.durationHours);
  const planned =
    Number.isFinite(hours) && hours > 0 ? new Date(start.getTime() + hours * 3_600_000) : null;
  const end = toDate(b.endTime) ?? planned ?? now;
  const originalEnd = toDate(b.originalEndTime) ?? planned;
  if (!toDate(b.startTime) && b.startTime) return { ok: false, error: "Invalid startTime" };
  return {
    ok: true,
    startTime: start,
    endTime: end,
    originalEndTime: originalEnd,
    isActive: typeof b.isActive === "boolean" ? b.isActive : true,
  };
}

export type FastPatchPlan =
  | { ok: true; set: { endTime?: Date; isActive?: boolean } }
  | { ok: false; error: string };

/**
 * A fast update. `{action: "end", id}` ends the fast now (the web's end button). The explicit
 * `endTime` and `isActive` fields still work.
 */
export function planFastPatch(body: unknown, now: Date): FastPatchPlan {
  const b = isRecord(body) ? body : {};
  if (b.action !== undefined && b.action !== "end") return { ok: false, error: "Unknown action" };
  if (b.action === "end") return { ok: true, set: { endTime: now, isActive: false } };
  const set: { endTime?: Date; isActive?: boolean } = {};
  if (b.endTime !== undefined) {
    const d = toDate(b.endTime);
    if (!d) return { ok: false, error: "Invalid endTime" };
    set.endTime = d;
  }
  if (typeof b.isActive === "boolean") set.isActive = b.isActive;
  if (Object.keys(set).length === 0) return { ok: false, error: "Nothing to update" };
  return { ok: true, set };
}

/**
 * The measurements or workouts document. The web posts the raw document; the native client
 * posts `{data}`. Both are accepted.
 */
export function documentPayload(body: unknown): unknown {
  if (isRecord(body) && "data" in body) return body.data;
  return body;
}
