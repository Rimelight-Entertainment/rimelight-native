import { describe, expect, it } from "vite-plus/test";
import { parseIds, planOwnedId } from "./owned.logic";

describe("planOwnedId", () => {
  it("returns the id scoped to the owner", () => {
    expect(planOwnedId(" n1 ", "owner")).toEqual({ ok: true, id: "n1", owner: "owner" });
  });

  it("rejects a missing or blank id", () => {
    expect(planOwnedId(undefined, "owner")).toEqual({ ok: false, error: "Missing id" });
    expect(planOwnedId("  ", "owner")).toEqual({ ok: false, error: "Missing id" });
  });
});

describe("parseIds", () => {
  it("reads a single id", () => {
    expect(parseIds("a", undefined)).toEqual(["a"]);
  });

  it("reads a comma list, dropping blanks and duplicates", () => {
    expect(parseIds(undefined, "a, b,,a,c")).toEqual(["a", "b", "c"]);
  });

  it("returns nothing when neither is set", () => {
    expect(parseIds(undefined, undefined)).toEqual([]);
  });
});
