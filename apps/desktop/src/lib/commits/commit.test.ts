import { shortRevisionId } from "$lib/commits/commit";
import { describe, expect, test } from "vitest";

describe("shortRevisionId", () => {
	test("uses the first three characters of a GitButler change ID", () => {
		expect(shortRevisionId({ id: "0123456789", changeId: "frpqrstuvwxyz" })).toBe("frp");
	});

	test("falls back to a seven-character Git commit ID", () => {
		expect(shortRevisionId({ id: "0123456789", changeId: null })).toBe("0123456");
	});
});
