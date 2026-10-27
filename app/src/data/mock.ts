import type { IconName } from "../lib/icons";

export type Category =
  | "Warframes"
  | "Primary"
  | "Secondary"
  | "Melee"
  | "Companions"
  | "Companion Weapons"
  | "Archwing"
  | "Archguns"
  | "Archmelee"
  | "Necramech"
  | "Amps"
  | "Mods"
  | "Arcanes"
  | "Relics"
  | "Ayatan";

export interface Part {
  icon: IconName;
  label: string;
  count: number;
}

export interface CatalogItem {
  id: string;
  name: string;
  category: Category;
  vaulted: boolean;
  owned: boolean;
  mastered: boolean;
  parts: Part[];
  setCount: number;
  price: number;
  ducats: number;
  hue: number;
}

export interface Cycle {
  id: string;
  name: string;
  icon: IconName;
  state: string;
  remaining: string;
  progress: number;
  tone: "blue" | "red" | "amber" | "green";
}

export interface Fissure {
  tier: string;
  mission: string;
  remaining: string;
  tone: "lith" | "meso" | "neo" | "axi" | "requiem";
}

export interface MarketItem {
  id: string;
  slug: string;
  name: string;
  price: number;
  online: number;
  delta: number;
  points: number[];
}

export interface InventoryRow {
  name: string;
  category: string;
  count: number;
  ducats: number;
  price: number;
  vaulted: boolean;
}

export interface Riven {
  id: string;
  weapon: string;
  name: string;
  rank: number;
  rerolls: number;
  polarity: string;
  price: number;
  stats: { label: string; value: string; positive: boolean }[];
}

export const categories: { id: Category; label: string }[] = [
  { id: "Warframes", label: "Warframe" },
  { id: "Primary", label: "Primary" },
  { id: "Secondary", label: "Secondary" },
  { id: "Melee", label: "Melee" },
  { id: "Companions", label: "Companion" },
  { id: "Companion Weapons", label: "Companion Weapons" },
  { id: "Archwing", label: "Archwing" },
  { id: "Archguns", label: "Archguns" },
  { id: "Archmelee", label: "Archmelee" },
  { id: "Necramech", label: "Necramech" },
  { id: "Amps", label: "Amps" },
];

const partSets: Record<Category, { icon: IconName; label: string }[]> = {
  Warframes: [
    { icon: "neuroptics", label: "Neuroptics" },
    { icon: "chassis", label: "Chassis" },
    { icon: "systems", label: "Systems" },
    { icon: "blueprint", label: "Blueprint" },
  ],
  Primary: [
    { icon: "barrel", label: "Barrel" },
    { icon: "receiver", label: "Receiver" },
    { icon: "stock", label: "Stock" },
    { icon: "blueprint", label: "Blueprint" },
  ],
  Secondary: [
    { icon: "barrel", label: "Barrel" },
    { icon: "receiver", label: "Receiver" },
    { icon: "blueprint", label: "Blueprint" },
  ],
  Melee: [
    { icon: "blade", label: "Blade" },
    { icon: "handle", label: "Handle" },
    { icon: "blueprint", label: "Blueprint" },
  ],
  Companions: [
    { icon: "chassis", label: "Carapace" },
    { icon: "systems", label: "Cerebrum" },
    { icon: "blueprint", label: "Blueprint" },
  ],
  "Companion Weapons": [
    { icon: "barrel", label: "Barrel" },
    { icon: "receiver", label: "Receiver" },
    { icon: "blueprint", label: "Blueprint" },
  ],
  Archwing: [
    { icon: "chassis", label: "Harness" },
    { icon: "blade", label: "Wings" },
    { icon: "systems", label: "Systems" },
    { icon: "blueprint", label: "Blueprint" },
  ],
  Archguns: [],
  Archmelee: [],
  Necramech: [
    { icon: "chassis", label: "Casing" },
    { icon: "systems", label: "Engine" },
    { icon: "blueprint", label: "Capsule" },
  ],
  Amps: [],
  Mods: [],
  Arcanes: [],
  Relics: [],
  Ayatan: [],
};

function slug(name: string) {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
}

function makeItem(
  name: string,
  category: Category,
  hue: number,
  vaulted = false,
  owned = false,
  mastered = owned,
): CatalogItem {
  const parts = partSets[category].map((part, index) => ({
    ...part,
    count: owned ? (index % 3 === 2 ? 2 : 1) : index === 3 ? 1 : 0,
  }));
  return {
    id: slug(name),
    name,
    category,
    vaulted,
    owned,
    mastered,
    parts,
    setCount: owned ? (hue % 2 === 0 ? 1 : 0) : 0,
    price: 14 + (hue % 53),
    ducats: category === "Warframes" ? 45 : 15 + (hue % 3) * 15,
    hue,
  };
}

const frameSeed: [string, number, boolean, boolean][] = [
  ["Limbo Prime", 218, true, true],
  ["Loki", 188, false, true],
  ["Loki Prime", 196, true, true],
  ["Mag", 310, false, true],
  ["Mag Prime", 322, false, true],
  ["Mesa", 28, false, true],
  ["Mesa Prime", 36, true, true],
  ["Mirage", 272, false, false],
  ["Mirage Prime", 282, true, false],
  ["Nekros", 148, false, true],
  ["Nekros Prime", 158, true, true],
  ["Nezha", 12, false, true],
  ["Nezha Prime", 18, true, true],
  ["Nidus", 110, false, true],
  ["Nidus Prime", 120, true, true],
  ["Nova", 248, false, false],
  ["Nova Prime", 258, true, true],
  ["Nyx", 334, false, true],
  ["Nyx Prime", 344, true, true],
  ["Oberon", 82, false, true],
  ["Oberon Prime", 88, true, true],
  ["Ash Prime", 206, true, true],
  ["Atlas Prime", 92, true, false],
  ["Banshee Prime", 232, true, true],
  ["Baruuk Prime", 44, true, false],
  ["Chroma Prime", 74, true, true],
  ["Dante", 264, false, true],
  ["Ember Prime", 14, true, true],
  ["Equinox Prime", 300, true, false],
  ["Frost Prime", 194, true, true],
  ["Gara Prime", 172, true, true],
  ["Garuda Prime", 348, true, false],
  ["Gauss Prime", 184, true, true],
  ["Grendel Prime", 104, true, false],
  ["Gyre Prime", 318, true, true],
  ["Harrow Prime", 268, true, true],
  ["Hildryn Prime", 34, true, false],
  ["Hydroid Prime", 210, true, true],
  ["Inaros Prime", 42, true, true],
  ["Ivara Prime", 286, true, true],
  ["Khora Prime", 142, true, false],
  ["Revenant Prime", 254, true, true],
  ["Rhino Prime", 24, true, true],
  ["Saryn Prime", 126, true, true],
  ["Sevagoth Prime", 174, true, true],
  ["Styanax Prime", 220, true, true],
  ["Titania Prime", 292, true, false],
  ["Trinity Prime", 204, true, true],
  ["Valkyr Prime", 356, true, true],
  ["Vauban Prime", 168, true, false],
  ["Volt Prime", 62, true, true],
  ["Wisp Prime", 276, true, true],
  ["Wukong Prime", 38, true, true],
  ["Xaku Prime", 304, true, false],
  ["Zephyr Prime", 154, true, true],
];

const weaponSeed: [string, Category, number, boolean, boolean][] = [
  ["Braton Prime", "Primary", 32, false, true],
  ["Paris Prime", "Primary", 152, true, true],
  ["Burston Prime", "Primary", 44, false, true],
  ["Strun Prime", "Primary", 20, false, true],
  ["Daikyu Prime", "Primary", 96, false, false],
  ["Perigale Prime", "Primary", 124, false, true],
  ["Cedo Prime", "Primary", 66, true, false],
  ["Soma Prime", "Primary", 208, true, true],
  ["Boltor Prime", "Primary", 224, true, true],
  ["Latron Prime", "Primary", 240, true, true],
  ["Lex Prime", "Secondary", 48, false, true],
  ["Bronco Prime", "Secondary", 84, true, true],
  ["Kompressa Prime", "Secondary", 132, false, true],
  ["Vadarya Prime", "Secondary", 166, false, true],
  ["Velox Prime", "Secondary", 194, true, false],
  ["Epitaph Prime", "Secondary", 214, true, true],
  ["Akstiletto Prime", "Secondary", 236, true, true],
  ["Sicarus Prime", "Secondary", 260, true, true],
  ["Dual Zoren Prime", "Melee", 26, true, true],
  ["Orthos Prime", "Melee", 58, true, true],
  ["Quassus Prime", "Melee", 102, false, true],
  ["Bo Prime", "Melee", 138, true, true],
  ["Nikana Prime", "Melee", 176, true, true],
  ["Glaive Prime", "Melee", 202, true, false],
  ["Kronen Prime", "Melee", 228, true, true],
  ["Reaper Prime", "Melee", 252, true, true],
  ["Wyrm Prime", "Companions", 212, true, true],
  ["Carrier Prime", "Companions", 190, true, true],
  ["Dethcube Prime", "Companions", 174, true, false],
  ["Shade Prime", "Companions", 246, true, true],
  ["Helios Prime", "Companions", 198, true, true],
  ["Diriga", "Companions", 116, false, true],
  ["Odonata Prime", "Archwing", 204, true, true],
  ["Itzal Prime", "Archwing", 238, true, true],
  ["Elytron Prime", "Archwing", 180, true, false],
  ["Corvas Prime", "Archwing", 52, false, true],
  ["Velocitus Prime", "Archwing", 264, true, true],
  ["Voidrig", "Necramech", 24, false, true],
  ["Bonewidow", "Necramech", 350, false, true],
];

export const catalog: CatalogItem[] = [
  ...frameSeed.map(([name, hue, vaulted, owned]) => makeItem(name, "Warframes", hue, vaulted, owned)),
  ...weaponSeed.map(([name, category, hue, vaulted, owned]) => makeItem(name, category, hue, vaulted, owned)),
];

export const cycles: Cycle[] = [
  { id: "earth", name: "Earth", icon: "globe", state: "Night", remaining: "1h 16m 12s", progress: 0.62, tone: "blue" },
  { id: "cetus", name: "Cetus", icon: "sun", state: "Night", remaining: "12m 22s", progress: 0.78, tone: "amber" },
  { id: "vallis", name: "Vallis", icon: "snowflake", state: "Cold", remaining: "10m", progress: 0.35, tone: "blue" },
  { id: "cambion", name: "Cambion", icon: "flame", state: "Vome", remaining: "12m 22s", progress: 0.54, tone: "red" },
];

export const fissures: Fissure[] = [
  { tier: "Lith", mission: "Mobile Def. (Eurasia, Earth)", remaining: "1h 25m", tone: "lith" },
  { tier: "Meso", mission: "Mobile Def. (Tharsis, Mars)", remaining: "1h 13m", tone: "meso" },
  { tier: "Meso", mission: "Defense (Casta, Ceres)", remaining: "46m 35s", tone: "meso" },
  { tier: "Neo", mission: "Defense (Belenus, Void)", remaining: "38m 40s", tone: "neo" },
  { tier: "Axi", mission: "Survival (Nimus, Eris)", remaining: "52m 1s", tone: "axi" },
  { tier: "Axi", mission: "Mobile Def. (Rotuma, Kuva Fort.)", remaining: "4m 58s", tone: "axi" },
  { tier: "Neo", mission: "Defense (Nabuk, Kuva Fort.)", remaining: "43m", tone: "neo" },
  { tier: "Requiem", mission: "Survival (Persto, Deimos)", remaining: "46m 11s", tone: "requiem" },
];

export const market: MarketItem[] = [
  { id: "braton_prime_barrel", slug: "braton_prime_barrel", name: "Braton Prime Barrel", price: 6, online: 24, delta: -2.1, points: [8, 7, 9, 6, 7, 5, 6] },
  { id: "lex_prime_blueprint", slug: "lex_prime_blueprint", name: "Lex Prime Blueprint", price: 11, online: 15, delta: 4.8, points: [7, 8, 8, 9, 10, 10, 11] },
  { id: "paris_prime_upper_limb", slug: "paris_prime_upper_limb", name: "Paris Prime Upper Limb", price: 8, online: 31, delta: -1.2, points: [10, 9, 8, 8, 7, 8, 8] },
  { id: "wukong_prime_neuroptics_blueprint", slug: "wukong_prime_neuroptics_blueprint", name: "Wukong Prime Neuroptics Blueprint", price: 46, online: 12, delta: 9.3, points: [32, 34, 31, 38, 41, 43, 46] },
  { id: "sevagoth_prime_neuroptics_blueprint", slug: "sevagoth_prime_neuroptics_blueprint", name: "Sevagoth Prime Neuroptics Blueprint", price: 20, online: 19, delta: 2.4, points: [15, 16, 18, 17, 19, 20, 20] },
  { id: "kompressa_prime_blueprint", slug: "kompressa_prime_blueprint", name: "Kompressa Prime Blueprint", price: 18, online: 9, delta: 5.1, points: [11, 13, 12, 14, 16, 17, 18] },
  { id: "dual_zoren_prime_blueprint", slug: "dual_zoren_prime_blueprint", name: "Dual Zoren Prime Blueprint", price: 22, online: 17, delta: -3.6, points: [27, 26, 25, 24, 23, 22, 22] },
  { id: "forma_blueprint", slug: "forma_blueprint", name: "Forma Blueprint", price: 0, online: 0, delta: 0, points: [0, 0, 0, 0, 0, 0, 0] },
];

export const inventory: InventoryRow[] = [
  { name: "Braton Prime Barrel", category: "Primary", count: 12, ducats: 15, price: 6, vaulted: false },
  { name: "Lex Prime Blueprint", category: "Secondary", count: 7, ducats: 15, price: 11, vaulted: false },
  { name: "Paris Prime Upper Limb", category: "Primary", count: 9, ducats: 15, price: 8, vaulted: true },
  { name: "Kronen Prime Blade", category: "Melee", count: 3, ducats: 45, price: 24, vaulted: false },
  { name: "Dual Zoren Prime Blueprint", category: "Melee", count: 2, ducats: 15, price: 22, vaulted: true },
  { name: "Sevagoth Prime Systems", category: "Warframe", count: 1, ducats: 45, price: 14, vaulted: false },
  { name: "Wukong Prime Chassis", category: "Warframe", count: 1, ducats: 15, price: 9, vaulted: true },
  { name: "Forma Blueprint", category: "Resource", count: 24, ducats: 0, price: 0, vaulted: false },
  { name: "Ayatan Anasa Sculpture", category: "Resource", count: 31, ducats: 0, price: 18, vaulted: false },
  { name: "Orokin Catalyst Blueprint", category: "Resource", count: 4, ducats: 0, price: 18, vaulted: false },
];

export const rivens: Riven[] = [
  {
    id: "r1",
    weapon: "Braton Prime",
    name: "Vexi-cronicon",
    rank: 8,
    rerolls: 12,
    polarity: "Madurai",
    price: 480,
    stats: [
      { label: "+Damage", value: "+142.3%", positive: true },
      { label: "+Multishot", value: "+91.4%", positive: true },
      { label: "-Zoom", value: "-42.1%", positive: false },
    ],
  },
  {
    id: "r2",
    weapon: "Lex Prime",
    name: "Lexi-zeti",
    rank: 8,
    rerolls: 3,
    polarity: "Naramon",
    price: 250,
    stats: [
      { label: "+Critical Chance", value: "+118.9%", positive: true },
      { label: "+Critical Damage", value: "+76.2%", positive: true },
      { label: "+Recoil", value: "+31.8%", positive: false },
    ],
  },
  {
    id: "r3",
    weapon: "Dual Zoren Prime",
    name: "Zoren-crita",
    rank: 8,
    rerolls: 27,
    polarity: "Vazarin",
    price: 720,
    stats: [
      { label: "+Critical Damage", value: "+164.7%", positive: true },
      { label: "+Attack Speed", value: "+42.3%", positive: true },
      { label: "-Combo Duration", value: "-28.4%", positive: false },
    ],
  },
  {
    id: "r4",
    weapon: "Paris Prime",
    name: "Paridex",
    rank: 8,
    rerolls: 41,
    polarity: "Madurai",
    price: 300,
    stats: [
      { label: "+Critical Chance", value: "+152.1%", positive: true },
      { label: "+Projectile Speed", value: "+86.4%", positive: true },
      { label: "-Impact", value: "-54.2%", positive: false },
    ],
  },
  {
    id: "r5",
    weapon: "Odonata Prime",
    name: "Odon-lex",
    rank: 8,
    rerolls: 0,
    polarity: "Naramon",
    price: 120,
    stats: [
      { label: "+Ability Strength", value: "+67.4%", positive: true },
      { label: "+Ability Range", value: "+51.2%", positive: true },
      { label: "-Efficiency", value: "-24.6%", positive: false },
    ],
  },
  {
    id: "r6",
    weapon: "Nekros Prime",
    name: "Nekro-auge",
    rank: 8,
    rerolls: 8,
    polarity: "Vazarin",
    price: 200,
    stats: [
      { label: "+Duration", value: "+94.8%", positive: true },
      { label: "+Strength", value: "+58.6%", positive: true },
      { label: "-Range", value: "-31.9%", positive: false },
    ],
  },
];

