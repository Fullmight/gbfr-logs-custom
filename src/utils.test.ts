import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { getSkillTranslationKeys, toHash, toHashString } from "./utils";

const englishUi = JSON.parse(readFileSync(resolve("src-tauri/lang/en/ui.json"), "utf8"));
const skillGroups = JSON.parse(readFileSync(resolve("src-tauri/assets/skill-groups.json"), "utf8"));

describe("utils", () => {
  it("toHash", () => {
    expect(toHash(1)).toBe("1");
    expect(toHash(255)).toBe("ff");
  });

  it("toHashString", () => {
    expect(toHashString(1)).toBe("00000001");
    expect(toHashString(255)).toBe("000000ff");
  });

  it("falls back from a game 2 skill variant to its ability slot", () => {
    expect(getSkillTranslationKeys("Pl2800", 1110)).toEqual(["skills.Pl2800.1110", "skills.Pl2800.1100"]);
  });

  it("does not merge unknown or legacy character action IDs", () => {
    expect(getSkillTranslationKeys("Pl2300", 1510)).toEqual(["skills.Pl2300.1510"]);
    expect(getSkillTranslationKeys({ Unknown: 123 }, 1101)).toEqual([]);
  });

  it("names the observed Cagliostro and Percival action IDs", () => {
    expect(englishUi.skills.Pl1800[1100]).toBe("Mehen");
    expect(englishUi.skills.Pl1800[1700]).toBe("Alexandria");
    expect(englishUi.skills.Pl1800[1800]).toBe("Pain Train");
    expect(englishUi.skills.Pl1000[40]).toBe("Zerreissen");
  });

  it("names and groups Fediel's complete basic attack chain", () => {
    const basicAttackIDs = Array.from({ length: 14 }, (_, index) => 100 + index);
    const additionalNormalAttackIDs = [120, 300, 301, 310, 400];
    const miasmaHandsIDs = [150, 151, 152, 153, 220, 250];
    const magicOrbFinisherIDs = [950, 952, 953, 954];

    expect(skillGroups.Pl2900["normal-attack"].skills).toEqual([
      ...basicAttackIDs,
      ...additionalNormalAttackIDs.slice(0, 1),
      ...miasmaHandsIDs,
      ...additionalNormalAttackIDs.slice(1),
      ...magicOrbFinisherIDs,
    ]);
    expect(englishUi.skills.Pl2900["skill-groups"]["normal-attack"]).toBe("Normal Attack");
    for (const [index, skillID] of basicAttackIDs.entries()) {
      expect(englishUi.skills.Pl2900[skillID]).toBe(`Attack ${index + 1}`);
    }
    expect(englishUi.skills.Pl2900[120]).toBe("Attack 15");
    expect(englishUi.skills.Pl2900[300]).toBe("Aerial Attack 1");
    expect(englishUi.skills.Pl2900[301]).toBe("Aerial Attack 2");
    expect(englishUi.skills.Pl2900[310]).toBe("Aerial Attack 3");
    expect(englishUi.skills.Pl2900[400]).toBe("Launch");
    for (const [index, skillID] of miasmaHandsIDs.entries()) {
      expect(englishUi.skills.Pl2900[skillID]).toBe(`Miasma Hands ${index + 1}`);
    }
    for (const [index, skillID] of magicOrbFinisherIDs.entries()) {
      expect(englishUi.skills.Pl2900[skillID]).toBe(`Combo Finisher (Magic Orb) ${index + 1}`);
    }
  });

  it("names Fediel's observed standalone actions", () => {
    expect(englishUi.skills.Pl2900[410]).toBe("Aerial Barrage");
    expect(englishUi.skills.Pl2900[7000]).toBe("Miasmic Abyss");
    expect(englishUi.skills.Pl2900[2010]).toBe("Claws of Reversal");
    expect(englishUi.skills.Pl2900[8010]).toBe("Sphere of Reversal");
  });

  it("contains no literal numeric skill placeholders in the English mappings", () => {
    for (const skills of Object.values(englishUi.skills) as Array<Record<string, unknown>>) {
      for (const name of Object.values(skills)) {
        if (typeof name === "string") expect(name).not.toMatch(/^Skill \d+$/);
      }
    }
  });
});
