// Which sidebar section is showing.

import { createContext, useContext } from "react";

export type Section = "identify" | "history" | "settings" | "about";

export interface NavValue {
  section: Section;
  go(section: Section): void;
}

export const NavContext = createContext<NavValue | null>(null);

export function useNav(): NavValue {
  const value = useContext(NavContext);
  if (!value) throw new Error("useNav must be used inside <NavContext.Provider>");
  return value;
}
