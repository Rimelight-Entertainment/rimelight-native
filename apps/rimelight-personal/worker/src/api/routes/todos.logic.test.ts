import { describe, expect, it } from "vite-plus/test";
import { planNewTodo, planTodoPost } from "./todos.logic";

describe("planTodoPost (todos POST)", () => {
  it("creates a todo when there is no action", () => {
    expect(planTodoPost({ title: "Buy milk" })).toEqual({ kind: "create" });
  });

  it("routes batchComplete to a batch over the ids, de-duplicated", () => {
    expect(planTodoPost({ action: "batchComplete", ids: ["a", "b", "a", ""] })).toEqual({
      kind: "batch",
      action: "batchComplete",
      ids: ["a", "b"],
    });
  });

  it("routes batchDelete to a batch", () => {
    expect(planTodoPost({ action: "batchDelete", ids: ["x"] })).toEqual({
      kind: "batch",
      action: "batchDelete",
      ids: ["x"],
    });
  });

  it("rejects a batch with no ids instead of creating a blank todo", () => {
    expect(planTodoPost({ action: "batchDelete" })).toEqual({ kind: "invalid", error: "Missing ids" });
    expect(planTodoPost({ action: "batchComplete", ids: [] })).toEqual({
      kind: "invalid",
      error: "Missing ids",
    });
  });

  it("rejects an unknown action", () => {
    expect(planTodoPost({ action: "nuke", ids: ["a"] })).toEqual({
      kind: "invalid",
      error: "Unknown action",
    });
  });
});

describe("planNewTodo (todos POST create)", () => {
  it("applies the defaults for category and priority", () => {
    expect(planNewTodo({ title: " Call " })).toEqual({
      ok: true,
      title: "Call",
      category: "General",
      priority: "medium",
      dueDate: null,
    });
  });

  it("requires a title", () => {
    expect(planNewTodo({})).toEqual({ ok: false, error: "Missing title" });
    expect(planNewTodo({ title: "   " })).toEqual({ ok: false, error: "Missing title" });
  });

  it("rejects an invalid due date", () => {
    expect(planNewTodo({ title: "x", dueDate: "not-a-date" })).toEqual({
      ok: false,
      error: "Invalid dueDate",
    });
  });
});
