// Which desktop conventions to follow. The UI runs in the system web view (WebKit on macOS,
// WebView2 on Windows), so the user agent names the OS. `?platform=mac|windows` overrides it,
// which lets both conventions be checked in one browser during development.

export type Platform = "mac" | "windows";

/** The platform for this window. */
export function detectPlatform(
  userAgent: string = typeof navigator === "undefined" ? "" : navigator.userAgent,
  search: string = typeof location === "undefined" ? "" : location.search,
): Platform {
  const override = new URLSearchParams(search).get("platform");
  if (override === "mac" || override === "windows") return override;
  return /Windows/i.test(userAgent) ? "windows" : "mac";
}

/**
 * Orders a dialog's buttons: macOS puts the default (primary) button last, on the right;
 * Windows puts it first, followed by the others (as in "OK  Cancel").
 */
export function orderButtons<T>(platform: Platform, others: T[], primary: T): T[] {
  return platform === "windows" ? [primary, ...others] : [...others, primary];
}

/** The modifier key label shown in shortcuts: ⌘ on macOS, Ctrl on Windows. */
export function modifierLabel(platform: Platform): string {
  return platform === "mac" ? "⌘" : "Ctrl+";
}
