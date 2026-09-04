import { describe, expect, it } from "vitest";
import { clockText, wordsText } from "./TimerFace";

describe("clockText", () => {
  it("pads both fields and never renders a negative clock", () => {
    expect(clockText(1500)).toBe("25:00");
    expect(clockText(65)).toBe("01:05");
    expect(clockText(9)).toBe("00:09");
    expect(clockText(0)).toBe("00:00");
    expect(clockText(-30)).toBe("00:00");
  });

  it("rounds up so a partial second still shows as remaining", () => {
    expect(clockText(59.4)).toBe("01:00");
  });
});

describe("wordsText", () => {
  it("sharpens only as the interval actually runs out", () => {
    expect(wordsText(0)).toBe("the interval is over");
    expect(wordsText(30)).toBe("less than a minute left");
    expect(wordsText(59)).toBe("less than a minute left");
    expect(wordsText(60)).toBe("about a minute left");
  });

  it("stays exact through the first ten minutes", () => {
    expect(wordsText(7 * 60)).toBe("about seven minutes left");
    expect(wordsText(10 * 60)).toBe("about ten minutes left");
  });

  it("rounds to the nearest five once there is plenty of time", () => {
    // 23 minutes reads as twenty-five: precision is what makes a clock urgent.
    expect(wordsText(23 * 60)).toBe("about twenty-five minutes left");
    expect(wordsText(12 * 60)).toBe("about ten minutes left");
    expect(wordsText(48 * 60)).toBe("about fifty minutes left");
  });

  it("spells compound tens with a hyphen", () => {
    expect(wordsText(35 * 60)).toBe("about thirty-five minutes left");
  });

  it("caps at an hour rather than inventing longer phrasings", () => {
    expect(wordsText(60 * 60)).toBe("an hour left");
    expect(wordsText(90 * 60)).toBe("over an hour left");
  });

  it("never rounds past the hour cap for a long interval", () => {
    // 58 minutes rounds to 60, which must read as the hour, not "sixty".
    expect(wordsText(58 * 60)).toBe("an hour left");
  });
});
