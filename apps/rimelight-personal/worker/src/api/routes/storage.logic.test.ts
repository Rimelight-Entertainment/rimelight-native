import { describe, expect, it } from "vite-plus/test";
import { cleanFolder, filesBase, planMove } from "./storage.logic";

const PUBLIC_BASE = filesBase("https://owner.example.workers.dev/api/storage/files");

describe("cleanFolder", () => {
  it("drops empty, dot and parent segments", () => {
    expect(cleanFolder("/music//scores/../drafts/")).toBe("music/scores/drafts");
    expect(cleanFolder("  ")).toBe("");
  });
});

describe("planMove (storage PATCH /files)", () => {
  const base = { currentKey: "music/take.mp3", currentFilename: "take.mp3", publicBase: PUBLIC_BASE };

  it("links to this instance, not a shared CDN", () => {
    expect(PUBLIC_BASE).toBe("https://owner.example.workers.dev/api/storage/object");
  });

  it("renames the basename and keeps the extension and folder", () => {
    expect(planMove({ ...base, basename: "final take" })).toEqual({
      ok: true,
      key: "music/final-take.mp3",
      filename: "final-take.mp3",
      url: `${PUBLIC_BASE}/music/final-take.mp3`,
      changed: true,
    });
  });

  it("keeps an explicit extension", () => {
    const plan = planMove({ ...base, basename: "cover.png" });
    expect(plan.ok && plan.filename).toBe("cover.png");
  });

  it("moves to another folder and keeps the name", () => {
    expect(planMove({ ...base, folder: "archive" })).toEqual({
      ok: true,
      key: "archive/take.mp3",
      filename: "take.mp3",
      url: `${PUBLIC_BASE}/archive/take.mp3`,
      changed: true,
    });
  });

  it("moves to the root when the folder is empty", () => {
    const plan = planMove({ ...base, folder: "" });
    expect(plan.ok && plan.key).toBe("take.mp3");
  });

  it("reports no change when nothing differs", () => {
    const plan = planMove({ ...base, folder: "music", basename: "take" });
    expect(plan.ok && plan.changed).toBe(false);
  });

  it("rejects an empty basename and a dot name", () => {
    expect(planMove({ ...base, basename: "   " })).toEqual({ ok: false, error: "Missing basename" });
    expect(planMove({ ...base, basename: "." })).toEqual({ ok: false, error: "Invalid name" });
  });
});
