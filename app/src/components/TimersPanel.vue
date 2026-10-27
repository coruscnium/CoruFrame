<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import Icon from "./Icon.vue";
import TierMark from "./TierMark.vue";
import type { IconName } from "../lib/icons";
import { cycles as mockCycles, fissures as mockFissures } from "../data/mock";
import { itemImage } from "../lib/equipment";
import type { WorldstateDto } from "../lib/api";
import { formatDuration } from "../lib/format";
import { cycleImages } from "../lib/locations";
import { ensureWorldstate, worldstateState } from "../stores/worldstate";

interface CycleCard {
  id: string;
  name: string;
  icon: IconName;
  image: string | null;
  state: string;
  remaining: string;
  // Fraction of the current window that is still left, so the bar drains with
  // the countdown instead of filling up as the state nears its change.
  progress: number;
  tone: "blue" | "red" | "amber" | "green";
}

interface FissureRow {
  tier: string;
  mission: string;
  remaining: string;
  steel: boolean;
  storm: boolean;
}

interface ResetRow {
  id: string;
  label: string;
  icon: IconName;
  remaining: string;
}

// Server resets: daily and weekly at 00:00 UTC; the Sortie and its syndicate
// alerts follow noon Eastern, which shifts with US daylight saving.
function nextDailyReset(ms: number) {
  const date = new Date(ms);
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate() + 1);
}

function easternOffsetMinutes(ms: number) {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: "America/New_York",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).formatToParts(new Date(ms));
  const value = (type: string) =>
    Number(parts.find((part) => part.type === type)?.value ?? 0);
  const wall = Date.UTC(
    value("year"),
    value("month") - 1,
    value("day"),
    value("hour") % 24,
    value("minute"),
    value("second"),
  );
  return Math.round((wall - ms) / 60_000);
}

function nextEasternNoon(ms: number) {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: "America/New_York",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    hour12: false,
  }).formatToParts(new Date(ms));
  const value = (type: string) =>
    Number(parts.find((part) => part.type === type)?.value ?? 0);
  const day = value("day") + (value("hour") % 24 >= 12 ? 1 : 0);
  const noonUtc = Date.UTC(value("year"), value("month") - 1, day, 12);
  return noonUtc - easternOffsetMinutes(noonUtc) * 60_000;
}

function nextWeeklyReset(ms: number) {
  const date = new Date(ms);
  const days = (8 - date.getUTCDay()) % 7 || 7;
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate() + days);
}

function secondsUntil(target: number, nowMs: number) {
  return Math.max(0, Math.floor((target - nowMs) / 1000));
}

const modeFilters = [
  { id: "all", label: "All" },
  { id: "normal", label: "Normal" },
  { id: "steel", label: "Steel Path" },
] as const;
// One mode switch drives both lists: the fissures and the Circuit share it.
const mode = ref<(typeof modeFilters)[number]["id"]>("all");

const live = computed(() => worldstateState.data);
const circuit = ref<{ normal: string[]; hard: string[] }>({ normal: [], hard: [] });
const now = ref(Math.floor(Date.now() / 1000));
let ticker: number | null = null;

// Keep the last rotation if an upstream hiccup returns an empty one.
watch(
  live,
  (data) => {
    if (data && (data.circuit.normal.length || data.circuit.hard.length)) {
      circuit.value = data.circuit;
    }
  },
  { immediate: true },
);

const showNormal = computed(() => mode.value !== "steel");
const showSteel = computed(() => mode.value !== "normal");
// While the community document is behind, keep the last rows on screen even
// after their countdown hits zero instead of emptying the section.
const stale = computed(() => live.value?.stale ?? false);

// The warframestat worldstate feed has been serving a stale document (cycles
// already expired). Project the four cycles forward with their known lengths
// so the panel keeps moving even while the upstream is stuck.
const CYCLE_SECONDS: Record<string, Record<string, number>> = {
  earth: { day: 4 * 3600, night: 4 * 3600 },
  cetus: { day: 100 * 60, night: 50 * 60 },
  // Vallis is warm for 6m40s and cold for 20m, not two equal halves.
  vallis: { warm: 400, cold: 20 * 60 },
  // Cambion shares the Plains' timer: Fass 100m, Vome 50m.
  cambion: { fass: 100 * 60, vome: 50 * 60 },
};

function nextState(id: string, state: string) {
  if (id === "earth" || id === "cetus") return state === "day" ? "night" : "day";
  if (id === "vallis") return state === "warm" ? "cold" : "warm";
  if (id === "cambion") return state === "fass" ? "vome" : "fass";
  return state;
}

function projectCycle(
  cycle: WorldstateDto["cycles"][number],
  nowMs: number,
): WorldstateDto["cycles"][number] {
  let end = Date.parse(cycle.expiry);
  if (Number.isNaN(end) || end > nowMs) return cycle;
  let state = cycle.state.toLowerCase();
  let start = end;
  let guard = 0;
  while (end <= nowMs && guard < 10_000) {
    state = nextState(cycle.id, state);
    const seconds = CYCLE_SECONDS[cycle.id]?.[state] ?? 0;
    if (!seconds) break;
    start = end;
    end += seconds * 1000;
    guard += 1;
  }
  return {
    ...cycle,
    state,
    activation: new Date(start).toISOString(),
    expiry: new Date(end).toISOString(),
  };
}

const cycles = computed(() =>
  (live.value?.cycles ?? []).map((cycle) => projectCycle(cycle, now.value * 1000)),
);



function secondsLeft(expiry: string) {
  const end = Date.parse(expiry);
  if (Number.isNaN(end)) return 0;
  return Math.max(0, Math.floor((end - now.value * 1000) / 1000));
}

// The three clocks the game runs on. The Sortie and its syndicate alerts
// follow noon Eastern, which shifts with US daylight saving.
const resetRows = computed<ResetRow[]>(() => {
  const nowMs = now.value * 1000;
  const resets = [
    {
      id: "daily",
      label: "Daily reset",
      icon: "clock" as IconName,
      target: nextDailyReset(nowMs),
    },
    {
      id: "sortie",
      label: "Sortie reset",
      icon: "zap" as IconName,
      target: nextEasternNoon(nowMs),
    },
    {
      id: "weekly",
      label: "Weekly reset",
      icon: "refresh" as IconName,
      target: nextWeeklyReset(nowMs),
    },
  ];
  return resets.map((reset) => ({
    ...reset,
    remaining: formatDuration(secondsUntil(reset.target, nowMs), false),
  }));
});

function cycleIcon(id: string, state: string): IconName {
  if (id === "cetus") return state === "day" ? "sun" : "moon";
  if (id === "vallis") return "snowflake";
  if (id === "cambion") return "flame";
  return "globe";
}

function cycleTone(id: string): CycleCard["tone"] {
  if (id === "cambion") return "red";
  if (id === "cetus") return "amber";
  return "blue";
}

const circuitImage = (name: string) => itemImage(name);

const alertRows = computed(() =>
  (live.value?.alerts ?? [])
    .filter((alert) => {
      const end = Date.parse(alert.expiry);
      return stale.value || Number.isNaN(end) || end > now.value * 1000;
    })
    .sort((left, right) => left.expiry.localeCompare(right.expiry))
    .map((alert) => ({
      key: `${alert.node}-${alert.expiry}`,
      mission: `${alert.mission_type} · ${alert.node}`,
      reward: alert.reward,
      remaining: formatDuration(secondsLeft(alert.expiry), false),
    })),
);

const cycleCards = computed<CycleCard[]>(() => {
  if (!live.value) {
    return mockCycles.map((cycle) => ({
      ...cycle,
      image: cycleImages[cycle.id] ?? null,
    }));
  }
  return cycles.value.map((cycle) => {
    const start = Date.parse(cycle.activation);
    const end = Date.parse(cycle.expiry);
    const total = end - start;
    const remaining = secondsLeft(cycle.expiry);
    const progress =
      total > 0 ? Math.min(1, Math.max(0, remaining / (total / 1000))) : 0;
    return {
      id: cycle.id,
      name: cycle.name,
      icon: cycleIcon(cycle.id, cycle.state),
      image: cycleImages[cycle.id] ?? null,
      state: cycle.state,
      remaining: cycle.expiry ? formatDuration(remaining) : cycle.time_left ?? "—",
      progress,
      tone: cycleTone(cycle.id),
    };
  });
});

const fissureRows = computed<FissureRow[]>(() => {
  const rows: FissureRow[] = live.value
    ? live.value.fissures
        .filter((fissure) => {
          const end = Date.parse(fissure.expiry);
          return stale.value || Number.isNaN(end) || end > now.value * 1000;
        })
        .map((fissure) => ({
          tier: fissure.tier,
          mission: `${fissure.mission_type} · ${fissure.node}`,
          remaining: formatDuration(secondsLeft(fissure.expiry), false),
          steel: fissure.is_hard,
          storm: fissure.is_storm,
        }))
    : mockFissures.map((fissure) => ({
        ...fissure,
        steel: false,
        storm: false,
      }));
  const filter = mode.value;
  const tierOrder: Record<string, number> = {
    lith: 0,
    meso: 1,
    neo: 2,
    axi: 3,
    requiem: 4,
    omnia: 5,
  };
  return rows
    .filter((row) => {
      if (filter === "all") return true;
      if (filter === "steel") return row.steel;
      return !row.steel;
    })
    .sort((left, right) => {
      const tier = (tierOrder[left.tier.toLowerCase()] ?? 99) - (tierOrder[right.tier.toLowerCase()] ?? 99);
      if (tier !== 0) return tier;
      const steel = Number(left.steel) - Number(right.steel);
      if (steel !== 0) return steel;
      const storm = Number(left.storm) - Number(right.storm);
      if (storm !== 0) return storm;
      return left.mission.localeCompare(right.mission);
    });
});

onMounted(() => {
  ensureWorldstate();
  ticker = window.setInterval(() => {
    now.value = Math.floor(Date.now() / 1000);
  }, 1000);
});

onUnmounted(() => {
  if (ticker) window.clearInterval(ticker);
});
</script>

<template>
  <div class="rail-panel">
    <div class="rail-section-head">
      <h2>Reset timers</h2>
    </div>
    <div class="alert-list">
      <div v-for="reset in resetRows" :key="reset.id" class="alert-row reset-row">
        <span class="alert-mark"><Icon :name="reset.icon" :size="13" /></span>
        <div class="fissure-body">
          <span class="fissure-mission">{{ reset.label }}</span>
        </div>
        <span class="fissure-time">{{ reset.remaining }}</span>
      </div>
    </div>

    <div class="rail-section-head">
      <h2>World cycles</h2>
    </div>
    <div class="cycle-grid">
      <div v-for="cycle in cycleCards" :key="cycle.id" class="cycle-card" :class="`tone-${cycle.tone}`">
        <div class="cycle-top">
          <img v-if="cycle.image" class="cycle-thumb" :src="cycle.image" alt="" referrerpolicy="no-referrer" loading="lazy" decoding="async" />
          <Icon v-else :name="cycle.icon" :size="15" />
          <span>{{ cycle.name }}</span>
        </div>
        <div class="cycle-state">{{ cycle.state }}</div>
        <div class="cycle-remaining">{{ cycle.remaining }}</div>
        <div
          class="progress"
          :title="`${Math.round(cycle.progress * 100)}% of this cycle left`"
          role="progressbar"
          :aria-valuenow="Math.round(cycle.progress * 100)"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-label="`${cycle.name} cycle time left`"
        >
          <span :style="{ width: `${Math.round(cycle.progress * 100)}%` }"></span>
        </div>
      </div>
    </div>

    <div class="rail-section-head">
      <h2>Alerts</h2>
      <span class="muted">{{ alertRows.length }} alerts</span>
    </div>
    <div class="alert-list">
      <div
        v-for="alert in alertRows"
        :key="alert.key"
        class="alert-row"
      >
        <span class="alert-mark"><Icon name="zap" :size="13" /></span>
        <div class="fissure-body">
          <span class="fissure-mission">{{ alert.mission }}</span>
          <span class="alert-reward">{{ alert.reward }}</span>
        </div>
        <span class="fissure-time">{{ alert.remaining }}</span>
      </div>
      <div v-if="!alertRows.length" class="market-empty">No active alerts.</div>
    </div>

    <div class="rail-section-head">
      <h2>Void fissures</h2>
      <span class="muted">{{ fissureRows.length }} fissures</span>
    </div>
    <div class="fissure-filters" role="tablist" aria-label="Fissure and circuit mode">
      <button
        v-for="option in modeFilters"
        :key="option.id"
        type="button"
        role="tab"
        class="fissure-filter"
        :class="{ active: mode === option.id }"
        :aria-selected="mode === option.id"
        @click="mode = option.id"
      >
        {{ option.label }}
      </button>
    </div>
    <div class="fissure-list">
      <div v-for="fissure in fissureRows" :key="`${fissure.tier}-${fissure.mission}-${fissure.steel}-${fissure.storm}`" class="fissure-row">
        <TierMark :tier="fissure.tier" />
        <div class="fissure-body">
          <span class="fissure-mission">{{ fissure.mission }}</span>
          <span class="fissure-tier">
            {{ fissure.tier }}<template v-if="fissure.steel"> · Steel</template><template v-if="fissure.storm"> · Storm</template>
          </span>
        </div>
        <span class="fissure-time">{{ fissure.remaining }}</span>
      </div>
      <div v-if="!fissureRows.length" class="market-empty">No active fissures.</div>
    </div>

    <div class="rail-section-head">
      <h2>The Circuit</h2>
    </div>
    <div class="fissure-filters" role="tablist" aria-label="Circuit mode">
      <button
        v-for="option in modeFilters"
        :key="option.id"
        type="button"
        role="tab"
        class="fissure-filter"
        :class="{ active: mode === option.id }"
        :aria-selected="mode === option.id"
        @click="mode = option.id"
      >
        {{ option.label }}
      </button>
    </div>
    <template v-if="showNormal">
      <div class="circuit-row">
        <div v-for="name in circuit.normal" :key="name" class="circuit-card">
          <span class="circuit-emblem">
            <img :src="circuitImage(name)" alt="" referrerpolicy="no-referrer" loading="lazy" decoding="async" />
          </span>
          <span class="circuit-name">{{ name }}</span>
        </div>
      </div>
      <div v-if="!circuit.normal.length" class="market-empty">No rotation available.</div>
    </template>
    <template v-if="showSteel">
      <div class="rail-section-head rail-subhead">
        <h2>Steel Path Circuit</h2>
      </div>
      <div class="circuit-row">
        <div v-for="name in circuit.hard" :key="name" class="circuit-card steel">
          <span class="circuit-emblem">
            <img :src="circuitImage(name)" alt="" referrerpolicy="no-referrer" loading="lazy" decoding="async" />
          </span>
          <span class="circuit-name">{{ name }}</span>
        </div>
      </div>
      <div v-if="!circuit.hard.length" class="market-empty">No rotation available.</div>
    </template>
  </div>
</template>
