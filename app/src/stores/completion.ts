import { computed } from "vue";
import { catalogStore, type CatalogCard } from "./catalog";
import { inventory } from "./inventory";

export const EQUIPMENT_KINDS = [
  "warframe",
  "primary",
  "secondary",
  "melee",
  "companion",
  "companion_weapon",
  "amp",
  "archwing",
  "necramech",
] as const;

export interface PartState {
  uniqueName: string;
  name: string;
  need: number;
  have: number;
  complete: boolean;
  marketSlug: string | null;
}

export interface CompletionItem {
  card: CatalogCard;
  owned: boolean;
  mastered: boolean;
  parts: PartState[];
}

export const completionItems = computed<CompletionItem[]>(() => {
  const snapshot = inventory.snapshot;
  const counts = snapshot?.component_counts ?? {};
  const owned = new Set(
    (snapshot?.items ?? [])
      .filter((item) => item.resolved)
      .map((item) => item.item.unique_name),
  );
  const mastered = new Set(snapshot?.mastery.mastered ?? []);

  return catalogStore.cards
    .filter((card) => (EQUIPMENT_KINDS as readonly string[]).includes(card.item.kind))
    .map((card) => {
      const parts: PartState[] = card.components.map((part) => {
        const need = part.count || 1;
        const have = counts[part.unique_name] ?? 0;
        return {
          uniqueName: part.unique_name,
          name: part.name,
          need,
          have,
          complete: have >= need,
          marketSlug: part.market_slug ?? null,
        };
      });
      return {
        card,
        owned: card.owned ?? owned.has(card.item.unique_name),
        mastered: card.mastered ?? mastered.has(card.item.unique_name),
        parts,
      };
    })
    .sort((left, right) => left.card.item.name.localeCompare(right.card.item.name));
});
