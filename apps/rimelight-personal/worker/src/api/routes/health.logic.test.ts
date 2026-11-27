import { describe, expect, it } from "vite-plus/test";
import { documentPayload, planFastPatch, planFastPost } from "./health.logic";

const now = new Date("2026-11-22T10:00:00.000Z");

describe("planFastPost", () => {
  it("uses durationHours to set the planned end", () => {
    const plan = planFastPost(
      { startTime: "2026-11-22T08:00:00.000Z", durationHours: 16 },
      now,
    );
    expect(plan.ok && plan.endTime.toISOString()).toBe("2026-11-23T00:00:00.000Z");
    expect(plan.ok && plan.originalEndTime?.toISOString()).toBe("2026-11-23T00:00:00.000Z");
    expect(plan.ok && plan.isActive).toBe(true);
  });

  it("keeps an explicit endTime over the duration", () => {
    const plan = planFastPost(
      { startTime: "2026-11-22T08:00:00.000Z", durationHours: 16, endTime: "2026-11-22T09:00:00.000Z" },
      now,
    );
    expect(plan.ok && plan.endTime.toISOString()).toBe("2026-11-22T09:00:00.000Z");
  });

  it("defaults the start to now", () => {
    const plan = planFastPost({}, now);
    expect(plan.ok && plan.startTime.toISOString()).toBe(now.toISOString());
  });

  it("rejects an unparseable startTime", () => {
    expect(planFastPost({ startTime: "soon" }, now)).toEqual({ ok: false, error: "Invalid startTime" });
  });
});

describe("planFastPatch", () => {
  it("ends the fast now for action end", () => {
    const plan = planFastPatch({ action: "end", id: "f1" }, now);
    expect(plan).toEqual({ ok: true, set: { endTime: now, isActive: false } });
  });

  it("still accepts explicit endTime and isActive", () => {
    const plan = planFastPatch({ endTime: "2026-11-22T09:00:00.000Z", isActive: false }, now);
    expect(plan.ok && plan.set.isActive).toBe(false);
    expect(plan.ok && plan.set.endTime?.toISOString()).toBe("2026-11-22T09:00:00.000Z");
  });

  it("rejects an unknown action and an empty update", () => {
    expect(planFastPatch({ action: "pause" }, now)).toEqual({ ok: false, error: "Unknown action" });
    expect(planFastPatch({}, now)).toEqual({ ok: false, error: "Nothing to update" });
  });
});

describe("documentPayload", () => {
  it("unwraps {data} from the native client", () => {
    expect(documentPayload({ data: { measurements: [1] } })).toEqual({ measurements: [1] });
  });

  it("takes the raw document the web posts", () => {
    expect(documentPayload({ measurements: [2] })).toEqual({ measurements: [2] });
  });
});
