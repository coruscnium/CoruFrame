import { ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { inventorySnapshotAvailable, isTauri } from "../lib/api";

const STORAGE_KEY = "coruframe.textScale";
const APP_SCALE_KEY = "coruframe.appScale";

export const TEXT_SCALES = [1, 1.1, 1.25, 1.5] as const;
export type TextScale = (typeof TEXT_SCALES)[number];

export const APP_SCALES = [1, 1.25, 1.5, 1.75, 2] as const;
export type AppScale = (typeof APP_SCALES)[number];

function storedScale(): TextScale {
  try {
    const value = Number(localStorage.getItem(STORAGE_KEY));
    return (TEXT_SCALES as readonly number[]).includes(value) ? (value as TextScale) : 1;
  } catch {
    return 1;
  }
}

function storedAppScale(): AppScale {
  try {
    const value = Number(localStorage.getItem(APP_SCALE_KEY));
    return (APP_SCALES as readonly number[]).includes(value) ? (value as AppScale) : 1;
  } catch {
    return 1;
  }
}

export const textScale = ref<TextScale>(storedScale());
export const appScale = ref<AppScale>(storedAppScale());

const SNAPSHOT_KEY = "coruframe.snapshot.enabled";
const SNAPSHOT_NOTICE_KEY = "coruframe.snapshot.notice";

type SnapshotPreference = "on" | "off" | null;

function storedPreference(): SnapshotPreference {
  try {
    const value = localStorage.getItem(SNAPSHOT_KEY);
    return value === "true" ? "on" : value === "false" ? "off" : null;
  } catch {
    return null;
  }
}

function persist(enabled: boolean) {
  try {
    localStorage.setItem(SNAPSHOT_KEY, enabled ? "true" : "false");
  } catch {
    /* storage unavailable */
  }
}

const initial = storedPreference();

// One owner for the opt-in flag. `checking` is true until the first preference
// bootstrap resolves, so nothing pretends the snapshot is off before we know.
export const snapshotEnabled = ref(initial === "on");
export const snapshotChecking = ref(initial === null);
export const snapshotNotice = ref(false);

let initPromise: Promise<void> | null = null;

// Fresh installs default off; an existing inventory-raw.json with no stored
// preference keeps working and gets a one-time notice.
export function initSnapshotPreference(): Promise<void> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    const stored = storedPreference();
    if (stored) {
      snapshotEnabled.value = stored === "on";
      snapshotChecking.value = false;
      return;
    }
    const available = await inventorySnapshotAvailable();
    snapshotEnabled.value = available;
    snapshotChecking.value = false;
    if (!available) return;
    persist(true);
    let noticeShown = false;
    try {
      noticeShown = localStorage.getItem(SNAPSHOT_NOTICE_KEY) === "shown";
      localStorage.setItem(SNAPSHOT_NOTICE_KEY, "shown");
    } catch {
      /* storage unavailable */
    }
    snapshotNotice.value = !noticeShown;
  })();
  return initPromise;
}

export function dismissSnapshotNotice() {
  snapshotNotice.value = false;
}

const AUTO_MARK_KEY = "coruframe.autoMarkTrades";

function storedAutoMark(): boolean {
  try {
    return localStorage.getItem(AUTO_MARK_KEY) !== "false";
  } catch {
    return true;
  }
}

export const autoMarkTrades = ref(storedAutoMark());

export function setAutoMarkTrades(value: boolean) {
  autoMarkTrades.value = value;
  try {
    localStorage.setItem(AUTO_MARK_KEY, value ? "true" : "false");
  } catch {
    /* storage unavailable */
  }
}

export function setSnapshotEnabled(enabled: boolean) {
  snapshotEnabled.value = enabled;
  snapshotChecking.value = false;
  persist(enabled);
}

export function applyTextScale() {
  document.documentElement.style.setProperty("--font-scale", String(textScale.value));
}

export function setTextScale(scale: TextScale) {
  textScale.value = scale;
  applyTextScale();
  try {
    localStorage.setItem(STORAGE_KEY, String(scale));
  } catch {
    /* storage unavailable */
  }
}

export async function applyAppScale() {
  if (!isTauri()) return;
  try {
    await getCurrentWebview().setZoom(appScale.value);
  } catch {
    /* webview not ready; the next launch applies it */
  }
}

export function setAppScale(scale: AppScale) {
  appScale.value = scale;
  void applyAppScale();
  try {
    localStorage.setItem(APP_SCALE_KEY, String(scale));
  } catch {
    /* storage unavailable */
  }
}
