import { reactive } from "vue";
import type { OrderFilter } from "../lib/api";
import type { Category } from "../data/mock";
import type { CatalogCard } from "./catalog";

export type RailTab = "timers" | "item" | "market";
export type CollectionLayout = "grid" | "list";
export type OwnershipFilter =
  | "all"
  | "owned"
  | "missing"
  | "unmastered"
  | "mastered"
  | "vaulted";

function storedLayout(): CollectionLayout {
  try {
    return localStorage.getItem("coruframe.collection.layout") === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
}

export const app = reactive({
  category: "Warframes" as Category,
  search: "",
  filter: "all" as OwnershipFilter,
  // Direction of the collection order: A-Z for name sorts, and ascending
  // mastery progress for the unmastered filter.
  sortAsc: true,
  layout: storedLayout(),
  railTab: "timers" as RailTab,
  railOpen: false,
  selected: null as CatalogCard | null,
  selectedFilter: null as OrderFilter | null,
  selectedNonce: 0,
  snapshotLinked: false,
  snapshotChecked: false,
});

export function selectItem(card: CatalogCard, filter?: OrderFilter | null) {
  app.selectedFilter = filter ?? null;
  app.selected = card;
  app.selectedNonce += 1;
  app.railTab = "item";
  if (window.innerWidth < 1220) app.railOpen = true;
}

export function clearSelection() {
  app.selected = null;
  app.selectedFilter = null;
  app.selectedNonce += 1;
}

export function toggleLayout() {
  app.layout = app.layout === "grid" ? "list" : "grid";
  try {
    localStorage.setItem("coruframe.collection.layout", app.layout);
  } catch {
    /* storage unavailable */
  }
}
