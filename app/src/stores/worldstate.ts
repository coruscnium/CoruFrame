import { shallowReactive } from "vue";
import { isTauri, worldstate, type WorldstateDto } from "../lib/api";

export const worldstateState = shallowReactive({
  data: null as WorldstateDto | null,
  loading: false,
  error: "",
});

let started = false;

export async function loadWorldstate(force = false) {
  if (worldstateState.loading) return;
  worldstateState.loading = true;
  worldstateState.error = "";
  try {
    const next = await worldstate(force);
    if (next) {
      worldstateState.data = next;
    } else if (isTauri()) {
      worldstateState.error = "unreachable";
    }
  } catch (error) {
    worldstateState.error = String(error);
  } finally {
    worldstateState.loading = false;
  }
}

// One poller for the whole app: the sidebar status and the rail panel read the
// same document, even while the rail is closed.
export function ensureWorldstate() {
  if (started) return;
  started = true;
  void loadWorldstate();
  window.setInterval(() => void loadWorldstate(), 60_000);
}
