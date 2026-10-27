import { shallowReactive } from "vue";
import {
  isTauri,
  marketAccountStatus,
  marketPresenceStatus,
  marketSetPresence,
  onAccountStatus,
  onPresence,
} from "../lib/api";

export type PresenceStatus = "online" | "ingame" | "invisible" | "auto";

const KEY = "coruframe.wfm.presence";

export const presence = shallowReactive({
  status: "invisible" as PresenceStatus,
  connected: false,
  error: "",
  effective: "",
});

let listening = false;
let applied = false;

function saved(): PresenceStatus {
  const value = localStorage.getItem(KEY);
  return value === "online" || value === "ingame" || value === "auto" ? value : "invisible";
}

async function apply(reconnect = false) {
  try {
    await marketSetPresence(presence.status, reconnect);
    applied = true;
  } catch (error) {
    presence.error = String(error);
  }
}

// Presence must never start the wallet load: only an already-connected account
// (or an explicit status selection) opens a socket.
async function applyWhenConnected(connected: boolean, reconnect = false) {
  if (!connected) {
    applied = false;
    presence.connected = false;
    presence.error = "";
    presence.effective = "";
    return;
  }
  if (presence.status === "invisible") return;
  await apply(reconnect);
}

function onPresenceEvent(state: {
  connected: boolean;
  error: string | null;
  effective: string | null;
}) {
  presence.connected = state.connected;
  presence.error = state.error ?? "";
  presence.effective = state.effective ?? "";
}

export async function initPresence() {
  presence.status = saved();
  if (!isTauri() || listening) return;
  listening = true;
  void onPresence(onPresenceEvent);
  void onAccountStatus((status) => void applyWhenConnected(status.connected));

  const state = await marketPresenceStatus();
  if (state) onPresenceEvent(state);
  // The account load may have finished before this subscription was attached.
  const account = await marketAccountStatus(false);
  if (account.connected) await applyWhenConnected(true);
  // The socket handshake can finish before the listener is attached.
  window.setTimeout(async () => {
    const later = await marketPresenceStatus();
    if (!later) return;
    onPresenceEvent(later);
  }, 1500);
}

export function setPresence(status: PresenceStatus) {
  presence.status = status;
  localStorage.setItem(KEY, status);
  void apply();
}

// A new account token needs a fresh socket sign-in.
export function restartPresence() {
  if (!applied) {
    void applyWhenConnected(true);
    return;
  }
  void apply(true);
}

export function stopPresence() {
  presence.connected = false;
  presence.error = "";
  presence.effective = "";
  applied = false;
  void marketSetPresence(null);
}
