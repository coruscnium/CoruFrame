<script setup lang="ts">
import { computed } from "vue";
import Icon from "./Icon.vue";
import ItemArt from "./ItemArt.vue";
import type { IconName } from "../lib/icons";
import { app, selectItem } from "../stores/app";
import { iconUrl } from "../stores/market";
import { wfcdImage } from "../lib/equipment";
import type { CatalogCard } from "../stores/catalog";

const props = defineProps<{
  card: CatalogCard;
  owned: boolean;
  mastered: boolean;
  componentCounts?: Record<string, number>;
}>();

const selected = computed(
  () => app.selected?.item.unique_name === props.card.item.unique_name,
);
const mastered = computed(() => props.mastered);
const parts = computed(() => props.card.components);
const marketArt = computed(() => {
  const marketItem = props.card.market;
  return marketItem ? iconUrl(marketItem.thumb ?? marketItem.icon) : null;
});
const masteryReq = computed(() => props.card.item.mastery_req ?? null);
// The item DB keeps one synthetic entry per kitgun slot; the suffix is the
// only signal the card needs, so no extra DTO field is required.
const kitgunSlot = computed(() => {
  if (props.card.item.item_type !== "Kitgun") return null;
  const name = props.card.item.unique_name;
  if (name.endsWith("#primary")) return "P";
  if (name.endsWith("#secondary")) return "S";
  return null;
});
const missingParts = computed(
  () =>
    props.card.components.filter(
      (part) => ownedOf(part.unique_name, part.count) < (part.count || 1),
    ).length,
);

function openCard() {
  selectItem(props.card);
}

function ownedOf(uniqueName: string, fallback: number) {
  const counts = props.componentCounts;
  if (!counts) return fallback;
  return counts[uniqueName] ?? 0;
}

function partImage(part: { image?: string | null }) {
  return wfcdImage(part.image);
}

function partIcon(uniqueName: string, name: string): IconName {
  const explicit = props.card.components.find((part) => part.unique_name === uniqueName)?.icon;
  if (explicit) return explicit as IconName;
  const lower = name.toLowerCase();
  if (lower.includes("blueprint")) return "blueprint";
  if (lower.includes("neuroptics") || lower.includes("helmet") || lower.includes("capsule"))
    return "neuroptics";
  if (lower.includes("chassis") || lower.includes("casing")) return "chassis";
  if (lower.includes("systems")) return "systems";
  if (lower.includes("barrel")) return "barrel";
  if (lower.includes("receiver")) return "receiver";
  if (lower.includes("stock")) return "stock";
  if (lower.includes("blade")) return "blade";
  if (lower.includes("handle")) return "handle";
  if (lower.includes("string")) return "string";
  if (lower.includes("limb")) return "limb";
  return "package";
}
</script>

<template>
  <article
    class="item-card"
    :class="{ owned, vaulted: card.item.vaulted, selected }"
    tabindex="0"
    role="button"
    :aria-label="`${card.item.name}, ${owned ? 'owned' : 'not owned'}${mastered ? ', mastered' : ''}${card.item.vaulted ? ', vaulted' : ''}`"
    @click="openCard"
    @keydown.enter.prevent="openCard"
    @keydown.space.prevent="openCard"
  >
    <div class="item-art">
      <ItemArt
        class="item-art-image"
        :image="card.item.image_url"
        :market-icon="marketArt"
        :kind="card.item.kind"
        :icon-size="26"
      />
    </div>

    <div class="item-body">
      <div class="item-title">
        <Icon name="star" class="mastery-star" :class="{ on: mastered }" :size="13" />
        <h3 :title="card.item.name">{{ card.item.name }}</h3>
        <span
          v-if="kitgunSlot"
          class="badge neutral"
          :title="kitgunSlot === 'P' ? 'Kitgun Primary' : 'Kitgun Secondary'"
          >{{ kitgunSlot }}</span
        >
        <span v-if="masteryReq && masteryReq > 0" class="badge neutral">MR {{ masteryReq }}</span>
      </div>
      <div v-if="parts.length" class="item-parts">
        <span
          v-for="part in parts"
          :key="part.unique_name"
          class="part-chip"
          :class="{ missing: ownedOf(part.unique_name, part.count) === 0 }"
          :title="`${part.name}: ${ownedOf(part.unique_name, part.count)} owned (needs ${part.count})`"
        >
          <img
            v-if="partImage(part)"
            class="part-chip-art"
            :src="partImage(part)!"
            alt=""
            loading="lazy"
            decoding="async"
            referrerpolicy="no-referrer"
          />
          <Icon v-else :name="partIcon(part.unique_name, part.name)" :size="14" />
          <span>{{ ownedOf(part.unique_name, part.count) }}</span>
        </span>
      </div>
      <div class="item-state">
        <span v-if="owned" class="state owned"><Icon name="check" :size="11" /> Owned</span>
        <span v-if="card.item.vaulted" class="state vaulted"
          ><Icon name="lock" :size="11" /> Vaulted</span
        >
        <span v-if="parts.length" class="state parts-left">
          {{ missingParts ? `${missingParts} missing` : "All parts" }}
        </span>
      </div>
    </div>
  </article>
</template>
