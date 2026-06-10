// Per-bundle app-icon loader. The fetch is async over IPC and the same icon is
// requested by many rows, so results are memoised by bundle id for the session.

import { api } from "./api";

const appIconRequests = new Map<string, Promise<string | null>>();

export function loadAppIcon(bundleId: string | null): Promise<string | null> {
  if (!bundleId) return Promise.resolve(null);
  const cached = appIconRequests.get(bundleId);
  if (cached) return cached;
  const request = api.appIcon(bundleId).catch(() => null);
  appIconRequests.set(bundleId, request);
  return request;
}
