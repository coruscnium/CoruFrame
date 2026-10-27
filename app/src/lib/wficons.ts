import warframe from "../assets/wf/IconWarframe_256.png";
import primary from "../assets/wf/IconRifle_256.png";
import secondary from "../assets/wf/IconPistol_256.png";
import melee from "../assets/wf/IconMelee_256.png";
import companion from "../assets/wf/IconSentinel_256.png";
import companionWeapon from "../assets/wf/IconWeapons_256.png";
import archwing from "../assets/wf/IconArchwing_256.png";
import archgun from "../assets/wf/IconArchwingPrimary_256.png";
import archmelee from "../assets/wf/IconArchwingSecondary_256.png";
import necramech from "../assets/wf/IconMech256.png";
import collection from "../assets/wf/IconWeapons_256.png";
import inventory from "../assets/wf/IconCapacity.png";
import allItems from "../assets/wf/IconAll_256.png";
import mods from "../assets/wf/IconMods_256.png";
import arcanes from "../assets/wf/IconArcaneMisc256.png";
import primeParts from "../assets/wf/IconPrimeParts256.png";
import misc from "../assets/wf/IconMisc_256.png";
import amp from "../assets/wf/IconMisc_256.png";
import relics from "../assets/wf/IconRelic256.png";
import rivens from "../assets/wf/IconOmegaMod256.png";
import trading from "../assets/wf/TradingIcon.png";

export type WfIconName =
  | "warframe"
  | "primary"
  | "secondary"
  | "melee"
  | "companion"
  | "companionWeapon"
  | "archwing"
  | "archgun"
  | "archmelee"
  | "necramech"
  | "amp"
  | "collection"
  | "inventory"
  | "allItems"
  | "mods"
  | "arcanes"
  | "primeParts"
  | "misc"
  | "relics"
  | "rivens"
  | "trading";

export const wfIcons: Record<WfIconName, string> = {
  warframe,
  primary,
  secondary,
  melee,
  companion,
  companionWeapon,
  archwing,
  archgun,
  archmelee,
  necramech,
  amp,
  collection,
  inventory,
  allItems,
  mods,
  arcanes,
  primeParts,
  misc,
  relics,
  rivens,
  trading,
};

const categoryIcons: Record<string, WfIconName> = {
  Warframes: "warframe",
  Primary: "primary",
  Secondary: "secondary",
  Melee: "melee",
  Companions: "companion",
  "Companion Weapons": "companionWeapon",
  Archwing: "archwing",
  Archguns: "archgun",
  Archmelee: "archmelee",
  Necramech: "necramech",
  Amps: "amp",
};

export function categoryIcon(categoryId: string): WfIconName {
  return categoryIcons[categoryId] ?? "collection";
}
