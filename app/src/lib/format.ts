export const MISSING = "—";

export function formatPlatinum(value: number | null | undefined, approx = false) {
  if (value == null) return MISSING;
  return `${approx ? "~" : ""}${value.toLocaleString()}p`;
}

export function formatDucats(value: number | null | undefined) {
  if (!value) return MISSING;
  return `${value.toLocaleString()}d`;
}

export function formatRank(value: number | null | undefined) {
  if (value == null) return MISSING;
  return `R${value}`;
}

export function formatPercent(value: number | null | undefined) {
  if (value == null) return MISSING;
  return `${value}%`;
}

export function formatCount(value: number | null | undefined) {
  if (value == null) return MISSING;
  return `×${value.toLocaleString()}`;
}

export function formatDuration(seconds: number, withSeconds = true) {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  if (hours) return `${hours}h ${minutes}m${withSeconds ? ` ${secs}s` : ""}`;
  if (minutes) return `${minutes}m${withSeconds ? ` ${secs}s` : ""}`;
  return `${secs}s`;
}

export function formatAge(seconds: number | null) {
  if (seconds === null) return "never";
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

// Clock skew must not produce a negative age ("-3s ago"); clamp at zero.
export function ageSince(at: number | null): number | null {
  if (!at) return null;
  return Math.max(0, Math.floor(Date.now() / 1000) - at);
}

// Today is HH:MM; another day keeps the date, so a day-old carry is not
// mistaken for a fresh read.
export function formatClock(at: number | null): string {
  if (!at) return "";
  const date = new Date(at * 1000);
  const today = new Date();
  const options: Intl.DateTimeFormatOptions =
    date.toDateString() === today.toDateString()
      ? { hour: "2-digit", minute: "2-digit" }
      : { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" };
  return date.toLocaleString([], options);
}
