<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import Icon from "../components/Icon.vue";
import Toggle from "../components/Toggle.vue";
import PageHeader from "../components/PageHeader.vue";
import {
  isTauri,
  formatAge,
  launchCommand,
  capabilities,
  hudSettings,
  setHudSettings,
  hudProbeStatus,
  startHudProbe,
  stopHudProbe,
  hudProbeLog,
  openHudLog,
  chooseCaptureSource,
  type CapabilitiesDto,
  type HudCorner,
  type HudProbeStatusDto,
  type HudSettingsDto,
} from "../lib/api";
import { ageSince } from "../lib/format";
import { catalogStore, loadItemDbStatus, updateItemDb } from "../stores/catalog";
import {
  account,
  connectAccount,
  disconnectAccount,
  loadAccount,
  loginAccount,
  refreshAccount,
} from "../stores/account";
import {
  inventory,
  loadInventory,
  rescanInventory,
  snapshotSchemaIsNewer,
} from "../stores/inventory";
import {
  APP_SCALES,
  TEXT_SCALES,
  appScale,
  autoMarkTrades,
  setAutoMarkTrades,
  setAppScale,
  setSnapshotEnabled,
  setTextScale,
  snapshotChecking,
  snapshotEnabled,
  textScale,
} from "../stores/ui";

const launchCommandText = ref("");
const caps = ref<CapabilitiesDto | null>(null);
const hud = ref<HudSettingsDto | null>(null);
const hudNotice = ref("");
const probe = ref<HudProbeStatusDto | null>(null);
const probeBusy = ref(false);
const probeError = ref("");
const probeLog = ref("");
const captureBusy = ref(false);
const captureNotice = ref("");
const captureError = ref("");
const corners: { value: HudCorner; label: string }[] = [
  { value: "top-left", label: "Top left" },
  { value: "top-right", label: "Top right" },
  { value: "bottom-left", label: "Bottom left" },
  { value: "bottom-right", label: "Bottom right" },
];
const emailInput = ref("");
const passwordInput = ref("");
const tokenInput = ref("");
const copied = ref<"" | "ok" | "fail">("");
const commandInput = ref<HTMLInputElement | null>(null);
const tauri = isTauri();

const snapshotSummary = computed(() => {
  const status = inventory.status;
  if (!status) {
    return inventory.snapshot
      ? "No scan status from this wrapper; showing the last changed snapshot."
      : "No scan yet.";
  }
  const scan = status.last_scan;
  const checkedAt = status.verified_at || status.last_scan_at;
  const tier = scan.fast_path === "none" ? "full" : scan.fast_path;
  const state = scan.error ?? scan.limit ?? (scan.complete ? "ok" : "partial");
  return `Scanned ${formatAge(ageSince(checkedAt))} · ${scan.trigger} · ${tier} · ${state}; snapshot changed ${formatAge(ageSince(status.captured_at))}.`;
});

const schemaNewer = computed(() => snapshotSchemaIsNewer());

function forceRewriteSnapshot() {
  void rescanInventory(true);
}

async function saveHud(corner: HudCorner, monitor: string | null) {
  const current = hud.value;
  if (!current) return;
  const previous = { corner: current.corner, monitor: current.monitor };
  current.corner = corner;
  current.monitor = monitor;
  if (await setHudSettings(corner, monitor)) return;
  current.corner = previous.corner;
  current.monitor = previous.monitor;
  hudNotice.value = "Could not save the HUD setting.";
}

function chooseCorner(corner: HudCorner) {
  void saveHud(corner, hud.value?.monitor ?? null);
}

const hudReady = computed(
  () => caps.value?.layer_shell === true && caps.value?.tesseract !== null,
);

async function refreshProbe() {
  probe.value = await hudProbeStatus();
}

async function toggleProbe(enabled: boolean) {
  if (probeBusy.value) return;
  probeBusy.value = true;
  probeError.value = "";
  probeLog.value = "";
  try {
    probe.value = enabled ? await startHudProbe() : await stopHudProbe();
    // A probe that dies on a missing grant or log exits within a second; read
    // the exit back so the error is not hidden behind a stale "running".
    if (enabled) window.setTimeout(() => void refreshProbe(), 1500);
  } catch (error) {
    probeError.value = String(error);
    await refreshProbe();
  } finally {
    probeBusy.value = false;
  }
}

async function showProbeLog() {
  probeLog.value = await hudProbeLog();
}

function chooseMonitor(event: Event) {
  const monitor = (event.target as HTMLSelectElement).value || null;
  void saveHud(hud.value?.corner ?? "top-right", monitor);
}

async function chooseSourceNow() {
  if (captureBusy.value) return;
  captureBusy.value = true;
  captureNotice.value = "";
  captureError.value = "";
  try {
    const result = await chooseCaptureSource();
    const streams = result.streams
      .map((stream) =>
        stream.width !== null && stream.height !== null
          ? `${stream.source} ${stream.width}×${stream.height}`
          : stream.source,
      )
      .join(", ");
    captureNotice.value = streams ? `Source set: ${streams}` : "No source selected.";
    caps.value = await capabilities();
  } catch (error) {
    captureError.value = String(error);
  } finally {
    captureBusy.value = false;
  }
}

const capabilityLines = computed(() => {
  const c = caps.value;
  if (!c) return [];
  const sources = [c.portal.monitor_source ? "monitor" : "", c.portal.window_source ? "window" : ""]
    .filter(Boolean)
    .join(" + ");
  const portalDetail = !c.portal.present
    ? "No portal"
    : !c.pipewire
      ? "No PipeWire"
      : `${c.portal.backend ?? "portal"}${sources ? ` · ${sources}` : ""}`;
  return [
    {
      name: "Wayland",
      ok: c.wayland,
      detail: c.wayland ? c.desktop || "Wayland" : "X11 unsupported",
    },
    {
      name: "Screen capture",
      ok: c.portal.present && c.pipewire && c.portal.source_types !== null,
      detail: portalDetail,
    },
    {
      name: "Reward HUD",
      ok: c.layer_shell === true,
      detail:
        c.layer_shell === true
          ? "layer-shell"
          : c.layer_shell === false
            ? "No layer-shell"
            : "Unknown",
    },
    {
      name: "Wallet",
      ok: c.secret_service,
      detail: c.secret_service ? "Secret Service" : "File fallback",
    },
    {
      name: "Inventory snapshot",
      ok: !c.snapshot.blocked,
      detail: c.snapshot.blocked
        ? "Blocked by ptrace_scope"
        : c.snapshot.present
          ? "Ready"
          : "No snapshot",
    },
    {
      name: "OCR",
      ok: c.tesseract !== null,
      detail: c.tesseract ?? "Missing",
    },
  ];
});

function loginNow() {
  if (!emailInput.value.trim() || !passwordInput.value) return;
  void loginAccount(emailInput.value, passwordInput.value).then((ok) => {
    if (ok) passwordInput.value = "";
  });
}

function connectAccountNow() {
  if (!tokenInput.value.trim()) return;
  void connectAccount(tokenInput.value).then((ok) => {
    if (ok) tokenInput.value = "";
  });
}

function toggleSnapshot(enabled: boolean) {
  setSnapshotEnabled(enabled);
  // Re-enabling reloads the snapshot once; disabling clears it in the store.
  void loadInventory(true);
}

function walletSummary() {
  if (account.status.storage === "unknown") return "Checking…";
  if (account.status.storage === "wallet") return "System wallet";
  if (account.status.storage === "file") return "Owner-only file";
  return "Not stored";
}

function disconnectAccountNow() {
  const ok = window.confirm(
    "Disconnect warframe.market? The token file is deleted and local My Orders data is cleared. Orders on the website are not touched.",
  );
  if (ok) void disconnectAccount();
}

// The webview origin is not a secure context on Linux, so the async Clipboard
// API can be missing; the selection fallback works either way.
async function copyLaunchCommand() {
  try {
    if (navigator.clipboard) {
      await navigator.clipboard.writeText(launchCommandText.value);
    } else {
      commandInput.value?.select();
      if (!document.execCommand("copy")) throw new Error("copy failed");
    }
    copied.value = "ok";
  } catch {
    copied.value = "fail";
  }
  window.setTimeout(() => {
    copied.value = "";
  }, 1600);
}

onMounted(() => {
  void loadAccount();
  void loadItemDbStatus();
  void capabilities().then((value) => {
    caps.value = value;
  });
  void hudSettings().then((value) => {
    hud.value = value;
  });
  void refreshProbe();
  void launchCommand().then((command) => {
    launchCommandText.value = command;
  });
});
</script>

<template>
  <div class="view">
    <div class="content">
      <div class="settings-page">
        <PageHeader
          title="Settings"
          description="Appearance, HUD, inventory and account."
          svg-icon="settings"
          rail-toggle
        />

        <div class="settings-grid">
          <section class="panel">
            <div class="panel-head"><h2>Appearance</h2><Icon name="sparkles" :size="16" /></div>
            <div class="field-row">
              <div class="field">
                <span>Text size</span>
                <div class="segmented" role="group" aria-label="Text size">
                  <button
                    v-for="scale in TEXT_SCALES"
                    :key="scale"
                    class="segmented-option"
                    :class="{ active: textScale === scale }"
                    type="button"
                    :aria-pressed="textScale === scale"
                    :aria-label="`Text size ${Math.round(scale * 100)} percent`"
                    @click="setTextScale(scale)"
                  >
                    {{ Math.round(scale * 100) }}%
                  </button>
                </div>
              </div>
              <div class="field">
                <span>App scale</span>
                <div class="segmented" role="group" aria-label="App scale">
                  <button
                    v-for="scale in APP_SCALES"
                    :key="scale"
                    class="segmented-option"
                    :class="{ active: appScale === scale }"
                    type="button"
                    :aria-pressed="appScale === scale"
                    :aria-label="`App scale ${Math.round(scale * 100)} percent`"
                    @click="setAppScale(scale)"
                  >
                    {{ Math.round(scale * 100) }}%
                  </button>
                </div>
              </div>
            </div>
          </section>

          <section class="panel">
            <div class="panel-head"><h2>Reward HUD</h2><Icon name="sparkles" :size="16" /></div>
            <Toggle
              :model-value="probe?.running ?? false"
              :disabled="!tauri || probeBusy || !hudReady"
              label="Reward HUD"
              description="Shows relic rewards after extraction."
              @update:model-value="toggleProbe"
            />
            <p v-if="caps && !hudReady" class="muted settings-note">
              Requires layer-shell and Tesseract.
            </p>
            <p v-else-if="probe?.running" class="muted settings-note">
              Running (pid {{ probe.pid }}).
            </p>
            <p v-else-if="probe?.enabled" class="muted settings-note">Enabled, not running.</p>
            <div v-if="probeError || probe?.last_error" class="warning-banner compact">
              <Icon name="shield" :size="15" />
              <div>
                {{ probeError || probe?.last_error }}
                <button class="button ghost" type="button" @click="showProbeLog">
                  Show log
                </button>
                <button class="button ghost" type="button" @click="openHudLog">
                  Open log file
                </button>
              </div>
            </div>
            <pre v-if="probeLog" class="probe-log">{{ probeLog }}</pre>
            <div class="field-row">
              <div class="field">
                <span>Corner</span>
                <div class="segmented" role="group" aria-label="HUD corner">
                  <button
                    v-for="corner in corners"
                    :key="corner.value"
                    class="segmented-option"
                    :class="{ active: hud?.corner === corner.value }"
                    type="button"
                    :aria-pressed="hud?.corner === corner.value"
                    :disabled="!hud"
                    @click="chooseCorner(corner.value)"
                  >
                    {{ corner.label }}
                  </button>
                </div>
              </div>
              <label class="field">
                <span>Monitor</span>
                <select :value="hud?.monitor ?? ''" :disabled="!hud" @change="chooseMonitor">
                  <option value="">Primary</option>
                  <option
                    v-for="monitor in hud?.monitors ?? []"
                    :key="monitor.connector"
                    :value="monitor.connector"
                  >
                    {{ monitor.model }} · {{ monitor.connector }} · {{ monitor.width }}×{{ monitor.height }} @ {{ monitor.scale }}x
                  </option>
                </select>
              </label>
            </div>
            <div class="cache-row">
              <div>
                <strong>Capture source</strong>
                <span>{{ caps?.capture_token ? "Grant stored" : "No grant" }}</span>
              </div>
              <button
                class="button ghost"
                type="button"
                :disabled="!tauri || captureBusy"
                @click="chooseSourceNow"
              >
                <Icon name="sparkles" :size="14" />
                {{ captureBusy ? "Waiting for the dialog…" : "Choose source…" }}
              </button>
            </div>
            <p v-if="captureNotice" class="muted settings-note">{{ captureNotice }}</p>
            <div v-if="captureError" class="warning-banner compact">
              <Icon name="shield" :size="15" />
              <div>{{ captureError }}</div>
            </div>
            <p v-if="hudNotice" class="muted settings-note">{{ hudNotice }}</p>
          </section>

          <section class="panel">
            <div class="panel-head"><h2>Inventory</h2><Icon name="database" :size="16" /></div>
            <Toggle
              :model-value="snapshotEnabled"
              :disabled="snapshotChecking"
              label="Inventory snapshot"
              description="Read-only scan while Warframe runs through the wrapper."
              @update:model-value="toggleSnapshot"
            />
            <p v-if="snapshotChecking" class="muted settings-note">Checking…</p>
            <label class="field">
              <span>Steam launch options</span>
              <div v-if="launchCommandText" class="copy-row">
                <input ref="commandInput" type="text" :value="launchCommandText" readonly />
                <button class="button ghost" type="button" @click="copyLaunchCommand">
                  <Icon :name="copied === 'ok' ? 'check' : 'copy'" :size="14" />
                  {{ copied === "ok" ? "Copied" : copied === "fail" ? "Select manually" : "Copy" }}
                </button>
              </div>
              <span v-else class="muted">Available in the installed app.</span>
            </label>
            <p class="muted settings-note">{{ snapshotSummary }}</p>
            <div class="cache-row">
              <div>
                <strong>Force rewrite</strong>
                <span>Rewrite the snapshot in case of errors or changes not being detected.</span>
              </div>
              <button
                class="button ghost"
                type="button"
                :disabled="!tauri || !snapshotEnabled || inventory.rescanning"
                title="Run a full scan and rewrite the raw snapshot"
                @click="forceRewriteSnapshot"
              >
                <Icon name="refresh" :size="14" :class="{ spinning: inventory.rescanning }" />
                {{ inventory.rescanning ? "Rewriting…" : "Force rewrite" }}
              </button>
            </div>
            <div v-if="schemaNewer" class="warning-banner compact">
              <Icon name="shield" :size="15" />
              <div>Snapshot format is newer; some sections may be ignored.</div>
            </div>
            <div class="cache-row">
              <div>
                <strong>
                  {{
                    !catalogStore.status
                      ? "Item data: checking…"
                      : catalogStore.status.available
                        ? `Item data: ${catalogStore.status.items.toLocaleString()} entries`
                        : "Item data: not built"
                  }}
                </strong>
                <span>
                  {{
                    catalogStore.status?.available
                      ? `Updated ${formatAge(catalogStore.status.age_secs)}`
                      : "Update to resolve items"
                  }}
                </span>
              </div>
              <button
                class="button ghost"
                type="button"
                :disabled="!tauri || catalogStore.updating"
                @click="updateItemDb(true)"
              >
                <Icon name="refresh" :size="14" />
                {{ catalogStore.updating ? "Updating…" : "Update" }}
              </button>
            </div>
            <div v-if="catalogStore.updateError" class="warning-banner compact">
              <Icon name="shield" :size="15" />
              <div>{{ catalogStore.updateError }}</div>
            </div>
          </section>

          <section class="panel">
            <div class="panel-head"><h2>Account</h2><Icon name="user" :size="16" /></div>
            <div class="cache-row">
              <div>
                <strong>
                  {{
                    account.status.connected
                      ? account.status.verified
                        ? "Connected"
                        : "Not verified"
                      : account.status.storage === "unknown"
                        ? "Checking…"
                        : "Not connected"
                  }}
                </strong>
                <span>{{ walletSummary() }}</span>
              </div>
              <span class="badge" :class="account.status.connected ? 'owned' : 'neutral'">
                {{ account.status.connected ? "Linked" : "Offline" }}
              </span>
            </div>
            <div v-if="account.status.warning" class="warning-banner compact">
              <Icon name="shield" :size="15" />
              <div>{{ account.status.warning }}</div>
            </div>
            <Toggle
              :model-value="autoMarkTrades"
              label="Auto-mark trades"
              description="Close a matching listing after a platinum trade."
              @update:model-value="setAutoMarkTrades"
            />
            <div v-if="account.status.needs_unlock" class="warning-banner compact">
              <Icon name="lock" :size="15" />
              <div>
                Wallet locked.
                <button class="button ghost" type="button" @click="refreshAccount()">
                  Retry
                </button>
              </div>
            </div>
            <div class="field-row">
              <label class="field">
                <span>Email or username</span>
                <input
                  v-model="emailInput"
                  type="text"
                  autocomplete="username"
                  spellcheck="false"
                  placeholder="you@example.com"
                  @keydown.enter.prevent="loginNow"
                />
              </label>
              <label class="field">
                <span>Password</span>
                <input
                  v-model="passwordInput"
                  type="password"
                  autocomplete="current-password"
                  placeholder="••••••••"
                  @keydown.enter.prevent="loginNow"
                />
              </label>
            </div>
            <div v-if="account.status.last_error" class="warning-banner compact">
              <Icon name="shield" :size="15" />
              <div>{{ account.status.last_error }}</div>
            </div>
            <div class="panel-actions">
              <button
                class="button primary"
                type="button"
                :disabled="!tauri || account.saving || !emailInput.trim() || !passwordInput"
                @click="loginNow"
              >
                <Icon name="user" :size="14" />
                {{ account.saving ? "Signing in…" : "Sign in" }}
              </button>
              <button
                v-if="account.status.connected"
                class="button danger"
                type="button"
                :disabled="account.saving"
                @click="disconnectAccountNow"
              >
                <Icon name="x" :size="14" /> Disconnect
              </button>
            </div>
            <details class="token-fallback">
              <summary>JWT token</summary>
              <p class="muted">Deprecated v1 token.</p>
              <label class="field">
                <span>JWT token</span>
                <input
                  v-model="tokenInput"
                  type="password"
                  autocomplete="off"
                  spellcheck="false"
                  placeholder="eyJhbGciOi…"
                  @keydown.enter.prevent="connectAccountNow"
                />
              </label>
              <div class="panel-actions">
                <button
                  class="button"
                  type="button"
                  :disabled="!tauri || account.saving || !tokenInput.trim()"
                  @click="connectAccountNow"
                >
                  {{ account.saving ? "Checking…" : "Connect" }}
                </button>
              </div>
            </details>
          </section>

          <section class="panel">
            <div class="panel-head"><h2>System</h2><Icon name="shield" :size="16" /></div>
            <div v-if="!caps" class="muted settings-note">Checking…</div>
            <div v-for="line in capabilityLines" :key="line.name" class="cache-row">
              <div>
                <strong>{{ line.name }}</strong>
                <span>{{ line.detail }}</span>
              </div>
              <span class="badge" :class="line.ok ? 'owned' : 'neutral'">
                {{ line.ok ? "Ready" : "Unavailable" }}
              </span>
            </div>
          </section>
        </div>
      </div>
    </div>
  </div>
</template>
