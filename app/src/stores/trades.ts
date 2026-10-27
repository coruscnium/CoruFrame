import { ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  isTauri,
  type MyOrderDto,
  type TradeEventDto,
  type TradeItemDto,
} from "../lib/api";
import { account, closeOrder, refreshOrders } from "./account";
import { autoMarkTrades } from "./ui";

export const tradeNotice = ref("");
let noticeTimer: number | null = null;
let unlisten: UnlistenFn | null = null;

function showNotice(text: string) {
  tradeNotice.value = text;
  if (noticeTimer) window.clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => {
    tradeNotice.value = "";
    noticeTimer = null;
  }, 12_000);
}

export function dismissTradeNotice() {
  tradeNotice.value = "";
  if (noticeTimer) {
    window.clearTimeout(noticeTimer);
    noticeTimer = null;
  }
}

function normalize(text: string) {
  return text.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function matches(order: MyOrderDto, item: TradeItemDto, kind: "sell" | "buy") {
  return (
    order.kind === kind &&
    order.item != null &&
    normalize(order.item.name) === normalize(item.name) &&
    (order.rank ?? null) === (item.rank ?? null)
  );
}

async function matchSide(
  items: TradeItemDto[],
  kind: "sell" | "buy",
  platinum: number,
  player: string,
) {
  if (!account.status.connected) {
    showNotice(`Trade with ${player}: connect warframe.market to match listings.`);
    return;
  }
  if (!account.orders.length) await refreshOrders();

  let changed = 0;
  for (const item of items) {
    const candidates = account.orders.filter((order) => matches(order, item, kind));
    if (!candidates.length) {
      showNotice(`Trade with ${player}: no ${kind} listing for ${item.name}.`);
      continue;
    }
    const order =
      candidates.find((candidate) => candidate.quantity >= item.quantity) ?? candidates[0];
    const sold = Math.min(item.quantity, order.quantity);
    if (!autoMarkTrades.value) {
      showNotice(`Matched ${item.name} ×${sold}; auto-mark is off.`);
      continue;
    }
    const ok = await closeOrder(order, sold);
    if (ok) {
      changed += 1;
      const price = platinum > 0 ? ` for ${platinum}p` : "";
      showNotice(`Marked ${item.name} ×${sold} sold${price}.`);
    } else {
      showNotice(`Could not mark ${item.name} sold: ${account.error}`);
    }
  }
  if (!changed && items.length > 1) {
    showNotice(`Trade with ${player}: no matching ${kind} listings were changed.`);
  }
}

async function handleTrade(event: TradeEventDto) {
  const offeredPlatinum = event.offering.find((item) => item.platinum)?.quantity ?? 0;
  const receivedPlatinum = event.receiving.find((item) => item.platinum)?.quantity ?? 0;
  const offered = event.offering.filter((item) => !item.platinum);
  const received = event.receiving.filter((item) => !item.platinum);

  if (offered.length > 0 && receivedPlatinum > 0) {
    await matchSide(offered, "sell", receivedPlatinum, event.player);
  } else if (received.length > 0 && offeredPlatinum > 0) {
    await matchSide(received, "buy", offeredPlatinum, event.player);
  } else {
    showNotice(`Trade with ${event.player} recorded; no platinum, no listing changed.`);
  }
}

export async function startTradeWatch() {
  if (!isTauri() || unlisten) return;
  unlisten = await listen<TradeEventDto>("trade-completed", (event) => {
    void handleTrade(event.payload);
  });
}

export function stopTradeWatch() {
  unlisten?.();
  unlisten = null;
}
