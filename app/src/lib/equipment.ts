import type { ItemDto } from "./api";
import type { IconName } from "./icons";

export type EquipmentKind =
  | "warframe"
  | "primary"
  | "secondary"
  | "melee"
  | "companion"
  | "archwing"
  | "archgun"
  | "archmelee"
  | "necramech";

// The item DB folds Archwing, Arch-Gun and Arch-Melee into one kind; the
// collection splits them using the WFCD item type.
export function equipmentKind(item: Pick<ItemDto, "kind" | "item_type">): string {
  if (item.kind === "archwing") {
    if (item.item_type === "Arch-Gun") return "archgun";
    if (item.item_type === "Arch-Melee") return "archmelee";
  }
  return item.kind;
}

// WFCD publishes the game's relic thumbnail per era and refinement:
// A = Exceptional, B = Flawless, C = Radiant, D = Intact.
const RELIC_ERA: Record<string, string> = {
  lith: "Lith",
  meso: "Meso",
  neo: "Neo",
  axi: "Axi",
  requiem: "Immortal",
};

const RELIC_REFINEMENT: Record<string, string> = {
  intact: "D",
  exceptional: "A",
  flawless: "B",
  radiant: "C",
};

// WFCD image names drop the spaces in display names ("Ash Prime" ->
// "AshPrime.png").
export function itemImage(name: string): string {
  return `https://cdn.warframestat.us/img/${encodeURIComponent(
    name.replace(/\s+/g, ""),
  )}.png`;
}

// One WFCD image file name, never a path; the CDN redirects to the GitHub host.
export function wfcdImage(image: string | null | undefined): string | null {
  const name = image?.trim();
  if (
    !name ||
    name.length > 200 ||
    name.startsWith(".") ||
    name.includes("..") ||
    !/^[A-Za-z0-9._'()-]+$/.test(name)
  ) {
    return null;
  }
  return `https://cdn.warframestat.us/img/${encodeURIComponent(name)}`;
}

export function relicImage(tier: string, refinement = "Intact"): string | null {
  const era = RELIC_ERA[tier.toLowerCase()];
  if (!era) return null;
  const variant = RELIC_REFINEMENT[refinement.toLowerCase()] ?? "D";
  return `https://cdn.warframestat.us/img/Relic${era}${variant}.png`;
}

export function kindIcon(kind: string | null | undefined): IconName {
  switch (kind) {
    case "warframe":
      return "warframe";
    case "primary":
    case "archgun":
      return "primary";
    case "secondary":
      return "secondary";
    case "melee":
    case "archmelee":
      return "melee";
    case "companion":
    case "companion_weapon":
      return "companion";
    case "amp":
      return "sparkles";
    case "archwing":
      return "archwing";
    case "necramech":
      return "necramech";
    case "mod":
    case "upgrade":
    case "recipe":
      return "blueprint";
    case "arcane":
      return "sparkles";
    case "relic":
      return "relics";
    default:
      return "package";
  }
}
