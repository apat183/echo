import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// The app formats dates with the *host* locale (`toLocaleDateString([], …)`),
// which is right for users and useless for assertions: the same code yields
// "Mon, Jun 8" on a US machine and "Mon, 8 Jun" on a British or NZ one. Pin the
// default here so date assertions can stay exact and pass on every machine.
// Note `[]` means "no preference" and is NOT nullish, so it needs handling too.
const TEST_LOCALE = "en-US";
const hostLocaleDate = Date.prototype.toLocaleDateString;
Date.prototype.toLocaleDateString = function (
  locales?: Intl.LocalesArgument,
  options?: Intl.DateTimeFormatOptions,
): string {
  const asked = Array.isArray(locales) ? locales.length > 0 : locales != null;
  return hostLocaleDate.call(this, asked ? locales : TEST_LOCALE, options);
};

afterEach(() => {
  cleanup();
});
