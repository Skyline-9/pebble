import { describe, expect, test } from "bun:test";
import { newCellId, newEventId, newSceneId } from "../src/ids";

describe("ids", () => {
  test("cell id is ULID-prefixed", () => {
    const id = newCellId();
    expect(id).toMatch(/^mc_[0-9A-HJKMNP-TV-Z]{26}$/);
  });
  test("event id is ULID-prefixed", () => {
    const id = newEventId();
    expect(id).toMatch(/^ev_[0-9A-HJKMNP-TV-Z]{26}$/);
  });
  test("scene id is slug-formatted", () => {
    const id = newSceneId("Auth Refactor");
    expect(id).toBe("ms_auth-refactor");
  });
  test("scene id dedups", () => {
    const id1 = newSceneId("Auth");
    const id2 = newSceneId("auth");
    expect(id1).toBe(id2);
  });
});
