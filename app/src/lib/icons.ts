export type IconName =
  | "inventory"
  | "relics"
  | "rivens"
  | "trading"
  | "stats"
  | "help"
  | "settings"
  | "warframe"
  | "primary"
  | "secondary"
  | "melee"
  | "companion"
  | "archwing"
  | "necramech"
  | "search"
  | "filter"
  | "star"
  | "amberStar"
  | "cyanStar"
  | "lock"
  | "check"
  | "chevronDown"
  | "refresh"
  | "clock"
  | "zap"
  | "shield"
  | "globe"
  | "snowflake"
  | "flame"
  | "package"
  | "external"
  | "play"
  | "sun"
  | "moon"
  | "x"
  | "arrowUp"
  | "arrowDown"
  | "plus"
  | "sparkles"
  | "neuroptics"
  | "chassis"
  | "systems"
  | "blueprint"
  | "barrel"
  | "receiver"
  | "stock"
  | "blade"
  | "handle"
  | "string"
  | "limb"
  | "set"
  | "crosshair"
  | "user"
  | "grid"
  | "list"
  | "menu"
  | "panelRight"
  | "copy"
  | "shop"
  | "terminal"
  | "database"
  | "signal";

export const icons: Record<IconName, string[]> = {
  inventory: [
    "M21 8l-9-5-9 5 9 5 9-5z",
    "M3 8v8l9 5 9-5V8",
    "M12 13v8",
  ],
  relics: ["M12 2.5l7 7-7 12-7-12z", "M9 9.5h6", "M12 6v11"],
  rivens: [
    "M6 3h12l3 5-9 13L3 8z",
    "M3 8h18",
    "M8.5 8L12 3l3.5 5L12 21z",
  ],
  trading: ["M3 17l6-6 4 4 8-8", "M15 7h6v6"],
  stats: ["M4 20V10", "M10 20V4", "M16 20v-7", "M22 20H2"],
  // Lucide "circle-question-mark" geometry (ISC, lucide.dev) in path form, so
  // the glyph is a maintained icon rather than a hand-drawn approximation.
  help: [
    "M12 2a10 10 0 1 0 0 20 10 10 0 1 0 0-20",
    "M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3",
    "M12 17h.01",
  ],
  settings: [
    "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7z",
    "M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.9.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.9V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  ],
  warframe: [
    "M12 3l5 2.5v5.2c0 4-2.2 7.1-5 8.3-2.8-1.2-5-4.3-5-8.3V5.5z",
    "M9.5 10.5h5",
    "M12 8v7",
  ],
  primary: ["M3 10h13l4 2v2H8l-5 3z", "M8 14v4", "M15 10V7h3"],
  secondary: ["M4 12h9l3 2v3H8l-4 2z", "M13 12V9h3", "M6 19v2"],
  melee: ["M4 20l9-9", "M14 4l6 6-3 3-6-6z", "M11 7l6 6"],
  companion: [
    "M7 13a3 3 0 0 1 3-3h4a3 3 0 0 1 3 3v2a4 4 0 0 1-4 4h-2a4 4 0 0 1-4-4z",
    "M9 10V7a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v3",
    "M9 15h.01",
    "M15 15h.01",
  ],
  archwing: ["M3 12l7-4v8z", "M21 12l-7-4v8z", "M10 12h4", "M12 4v4", "M12 16v4"],
  necramech: ["M5 9h14v8H5z", "M9 9V5h6v4", "M8 17v3", "M16 17v3", "M9 12h.01", "M15 12h.01"],
  search: ["M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z", "M21 21l-4.3-4.3"],
  filter: ["M3 5h18", "M6 12h12", "M10 19h4"],
  star: ["M12 2.6l2.9 5.9 6.5.9-4.7 4.6 1.1 6.5L12 17.4l-5.8 3.1 1.1-6.5L2.6 9.4l6.5-.9z"],
  amberStar: ["M12 2l2.4 7.6L22 12l-7.6 2.4L12 22l-2.4-7.6L2 12l7.6-2.4z"],
  cyanStar: ["M12 2l2.4 7.6L22 12l-7.6 2.4L12 22l-2.4-7.6L2 12l7.6-2.4z"],
  lock: ["M6 11h12v9H6z", "M8 11V8a4 4 0 0 1 8 0v3"],
  check: ["M4 12.5l5 5L20 6.5"],
  chevronDown: ["M6 9l6 6 6-6"],
  refresh: ["M20 11a8 8 0 1 0-2 5.3", "M20 4v7h-7"],
  clock: ["M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z", "M12 6v6l4 2"],
  zap: ["M13 2L4 14h6l-1 8 9-12h-6z"],
  shield: ["M12 3l8 3v6c0 5-3.4 8.2-8 9.5C7.4 20.2 4 17 4 12V6z"],
  globe: ["M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z", "M2 12h20", "M12 2c3 3 3 17 0 20", "M12 2c-3 3-3 17 0 20"],
  snowflake: ["M12 2v20", "M4 6l16 12", "M20 6L4 18", "M9 4l3 3 3-3", "M9 20l3-3 3 3"],
  flame: ["M12 22c4 0 7-2.7 7-6.5 0-3.5-2.5-5.5-4-8.5-.7 2-1.5 3-2.5 4-.5-2-1.8-4.5-3-6-1.2 3.5-4.5 6.5-4.5 10.5C5 19.3 8 22 12 22z"],
  package: ["M21 8l-9-5-9 5 9 5 9-5z", "M3 8v8l9 5 9-5V8", "M12 13v8"],
  external: ["M14 4h6v6", "M20 4l-9 9", "M18 13v6H4V5h6"],
  play: ["M8 5l12 7-12 7z"],
  sun: ["M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10z", "M12 1v2", "M12 21v2", "M4.2 4.2l1.4 1.4", "M18.4 18.4l1.4 1.4", "M1 12h2", "M21 12h2", "M4.2 19.8l1.4-1.4", "M18.4 5.6l1.4-1.4"],
  moon: ["M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"],
  x: ["M6 6l12 12", "M18 6L6 18"],
  arrowUp: ["M12 19V5", "M6 11l6-6 6 6"],
  arrowDown: ["M12 5v14", "M6 13l6 6 6-6"],
  plus: ["M12 5v14", "M5 12h14"],
  sparkles: ["M12 3l1.7 5.3L19 10l-5.3 1.7L12 17l-1.7-5.3L5 10l5.3-1.7z", "M19 3v3", "M20.5 4.5h-3"],
  neuroptics: ["M5 9a7 7 0 0 1 14 0v6a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3z", "M8 9h2", "M14 9h2", "M9 14h6"],
  chassis: ["M5 7l7-4 7 4v10l-7 4-7-4z", "M5 7l7 4 7-4", "M12 11v10"],
  systems: ["M7 7h10v10H7z", "M9 3v4", "M15 3v4", "M9 17v4", "M15 17v4", "M3 9h4", "M17 9h4", "M3 15h4", "M17 15h4"],
  blueprint: ["M6 2h9l4 4v16H6z", "M15 2v5h5", "M9 12h6", "M9 16h6", "M9 8h2"],
  barrel: ["M3 9h18v6H3z", "M6 9v6", "M18 9v6", "M12 9v6"],
  receiver: ["M4 8h16v8H4z", "M8 8v8", "M16 8v8", "M11 12h2"],
  stock: ["M3 10h8l4-3h6v10h-6l-4-3H3z", "M7 10v4"],
  blade: ["M4 20L18 4l3 3L7 21z", "M14 8l2 2"],
  handle: ["M4 16h10v4H4z", "M14 16l6-6", "M16 8l4 4"],
  string: ["M4 20c8 0 16-8 16-16", "M3 21l3-3", "M19 3l2 2"],
  limb: ["M5 19c0-8 6-14 14-14", "M5 19h6", "M16 3v4"],
  set: ["M12 2l5 3v6l-5 3-5-3V5z", "M7 14l5 3 5-3v6l-5 3-5-3z"],
  crosshair: ["M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z", "M12 2v4", "M12 18v4", "M2 12h4", "M18 12h4"],
  user: ["M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2", "M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8z"],
  grid: ["M3 3h8v8H3z", "M13 3h8v8h-8z", "M3 13h8v8H3z", "M13 13h8v8h-8z"],
  list: ["M8 6h13", "M8 12h13", "M8 18h13", "M3 6h.01", "M3 12h.01", "M3 18h.01"],
  menu: ["M4 7h16", "M4 12h16", "M4 17h16"],
  panelRight: ["M3 4h18v16H3z", "M15 4v16"],
  copy: ["M8 8h12v12H8z", "M4 16V4h12"],
  shop: ["M3 9l2-5h14l2 5", "M4 9v11h16V9", "M9 20v-6h6v6", "M3 9h18"],
  terminal: ["M4 4h16v16H4z", "M8 9l3 3-3 3", "M13 15h4"],
  database: ["M4 6c0-2 4-3 8-3s8 1 8 3-4 3-8 3-8-1-8-3z", "M4 6v6c0 2 4 3 8 3s8-1 8-3V6", "M4 12v6c0 2 4 3 8 3s8-1 8-3v-6"],
  signal: ["M2 20h.01", "M7 20v-4", "M12 20v-8", "M17 20V8", "M22 4v16"],
};

export const filledIcons = new Set<IconName>([
  "star",
  "play",
  "zap",
  "amberStar",
  "cyanStar",
]);
