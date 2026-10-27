import { reactive } from "vue";
import {
  isTauri,
  marketAccountConnect,
  marketAccountDisconnect,
  marketAccountLoad,
  marketAccountLogin,
  marketAccountStatus,
  marketMyOrders,
  marketOrderClose,
  marketOrderCreate,
  marketOrderDelete,
  marketOrderUpdate,
  onAccountStatus,
  type AccountStatusDto,
  type MyOrderDto,
  type OrderInputDto,
} from "../lib/api";
import { restartPresence, stopPresence } from "./presence";

export const account = reactive({
  status: {
    connected: false,
    verified: false,
    last_error: null,
    storage: "unknown",
    needs_unlock: true,
    warning: null,
  } as AccountStatusDto,
  orders: [] as MyOrderDto[],
  loading: false,
  saving: false,
  error: "",
  lastOrderId: null as string | null,
  initialized: false,
});

let subscribed = false;

async function applyStatus(status: AccountStatusDto) {
  account.status = status;
  if (status.connected) await refreshOrders();
}

// Subscribe before the load so the background result cannot be missed, then
// share one single-flight load with presence and the first market action.
async function subscribe() {
  if (subscribed || !isTauri()) return;
  subscribed = true;
  void onAccountStatus((status) => void applyStatus(status));
}

export async function loadAccount() {
  if (account.loading) return;
  account.loading = true;
  if (!isTauri()) {
    account.status = {
      ...account.status,
      connected: false,
      storage: "none",
      needs_unlock: false,
    };
    account.loading = false;
    account.initialized = true;
    return;
  }
  await subscribe();
  try {
    await applyStatus(await marketAccountLoad(false));
  } catch (error) {
    account.status = { ...account.status, last_error: String(error) };
  } finally {
    account.loading = false;
    account.initialized = true;
  }
}

// The sidebar/settings Refresh is a manual action, so it bypasses the
// account-configured marker and retries a locked wallet.
export async function refreshAccount() {
  if (!isTauri() || account.saving) return;
  account.loading = true;
  try {
    await subscribe();
    await applyStatus(await marketAccountLoad(true));
  } catch (error) {
    account.error = String(error);
    account.status = { ...account.status, last_error: String(error) };
  } finally {
    account.loading = false;
    account.initialized = true;
  }
}

export async function loginAccount(email: string, password: string) {
  account.saving = true;
  account.error = "";
  try {
    account.status = await marketAccountLogin(email, password);
    if (account.status.connected) {
      restartPresence();
      await refreshOrders();
    }
    return account.status.connected;
  } catch (error) {
    account.status = { ...account.status, last_error: String(error) };
    return false;
  } finally {
    account.saving = false;
    account.initialized = true;
  }
}

export async function connectAccount(token: string) {
  account.saving = true;
  account.error = "";
  try {
    account.status = await marketAccountConnect(token);
    if (account.status.connected) restartPresence();
    await refreshOrders();
    return account.status.connected;
  } catch (error) {
    account.status = { ...account.status, last_error: String(error) };
    return false;
  } finally {
    account.saving = false;
    account.initialized = true;
  }
}

export async function disconnectAccount() {
  account.saving = true;
  try {
    account.status = await marketAccountDisconnect();
    account.orders = [];
    account.initialized = true;
    stopPresence();
  } catch (error) {
    account.error = String(error);
  } finally {
    account.saving = false;
  }
}

// Kept for callers that only need to re-read the non-secret state.
export async function reloadAccountStatus() {
  account.status = await marketAccountStatus(false);
}

export async function refreshOrders() {
  if (!isTauri() || !account.status.connected) {
    account.orders = [];
    return;
  }
  try {
    account.orders = await marketMyOrders();
    account.error = "";
    account.status = { ...account.status, verified: true, last_error: null };
  } catch (error) {
    account.error = String(error);
    account.status = {
      ...account.status,
      verified: false,
      last_error: String(error),
    };
  }
}

function inputOf(order: MyOrderDto, changes: Partial<OrderInputDto> = {}): OrderInputDto {
  return {
    item_id: order.item_id,
    platinum: order.platinum,
    quantity: order.quantity,
    visible: order.visible,
    rank: order.rank,
    charges: order.charges,
    amber_stars: order.amber_stars,
    cyan_stars: order.cyan_stars,
    subtype: order.subtype,
    per_trade: order.per_trade,
    ...changes,
  };
}

async function write(action: () => Promise<void>) {
  account.saving = true;
  account.error = "";
  try {
    await action();
    await refreshOrders();
    return true;
  } catch (error) {
    account.error = String(error);
    return false;
  } finally {
    account.saving = false;
  }
}

export async function createOrder(input: OrderInputDto) {
  account.saving = true;
  account.error = "";
  account.lastOrderId = null;
  try {
    account.lastOrderId = await marketOrderCreate(input);
    await refreshOrders();
    return true;
  } catch (error) {
    account.error = String(error);
    return false;
  } finally {
    account.saving = false;
  }
}

export function updateOrder(order: MyOrderDto, changes: Partial<OrderInputDto>) {
  return write(() => marketOrderUpdate(order.id, inputOf(order, changes)));
}

export function setVisible(order: MyOrderDto, visible: boolean) {
  return updateOrder(order, { visible });
}

export function closeOrder(order: MyOrderDto, quantity: number) {
  return write(() => marketOrderClose(order.id, quantity));
}

export function deleteOrder(order: MyOrderDto) {
  return write(() => marketOrderDelete(order.id));
}

export function listedValue() {
  return account.orders
    .filter((order) => order.kind === "sell")
    .reduce((sum, order) => {
      const perTrade = order.per_trade ?? 1;
      const unit = perTrade > 1 ? Math.floor(order.platinum / perTrade) : order.platinum;
      return sum + unit * order.quantity;
    }, 0);
}

export function openSellCount() {
  return account.orders.filter((order) => order.kind === "sell" && order.visible).length;
}
