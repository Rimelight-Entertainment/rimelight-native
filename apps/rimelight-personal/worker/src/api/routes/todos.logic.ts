export type BatchAction = "batchComplete" | "batchDelete";

export type TodoPostPlan =
  | { kind: "create" }
  | { kind: "batch"; action: BatchAction; ids: string[] }
  | { kind: "invalid"; error: string };

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/**
 * Decides what `POST /api/todos` does. A body with an `action` is a batch call
 * (`batchComplete` or `batchDelete` over `ids`); any other body creates a todo. An unknown
 * action is rejected instead of creating a blank todo.
 */
export function planTodoPost(body: unknown): TodoPostPlan {
  if (!isRecord(body) || body.action === undefined) return { kind: "create" };
  const { action } = body;
  if (action !== "batchComplete" && action !== "batchDelete") {
    return { kind: "invalid", error: "Unknown action" };
  }
  const ids = Array.isArray(body.ids)
    ? [...new Set(body.ids.filter((x): x is string => typeof x === "string" && x.length > 0))]
    : [];
  if (ids.length === 0) return { kind: "invalid", error: "Missing ids" };
  return { kind: "batch", action, ids };
}

export type NewTodoPlan =
  | { ok: true; title: string; category: string; priority: string; dueDate: Date | null }
  | { ok: false; error: string };

/** Validates the body of a create. A todo needs a non-empty title. */
export function planNewTodo(body: unknown): NewTodoPlan {
  const b = isRecord(body) ? body : {};
  const title = typeof b.title === "string" ? b.title.trim() : "";
  if (!title) return { ok: false, error: "Missing title" };
  let dueDate: Date | null = null;
  if (b.dueDate) {
    const parsed = new Date(b.dueDate as string);
    if (Number.isNaN(parsed.getTime())) return { ok: false, error: "Invalid dueDate" };
    dueDate = parsed;
  }
  return {
    ok: true,
    title,
    category: typeof b.category === "string" && b.category ? b.category : "General",
    priority: typeof b.priority === "string" && b.priority ? b.priority : "medium",
    dueDate,
  };
}
