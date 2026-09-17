import { useEffect } from "react";

const MENU_SELECTOR = "details.row-menu";
const GAP = 2;
const MARGIN = 8;
/**
 * Room to ask for below the button. A menu can grow after it opens — deleting
 * a captured notification swaps the actions for a confirmation — so the
 * direction is chosen for the tallest one, not for what is measured on open.
 */
const TALLEST_MENU = 176;

export interface MenuPlacement {
  /** Distance from the viewport's top edge; set when the menu opens downwards. */
  top: number | null;
  /** Distance from the viewport's bottom edge; set when the menu opens upwards. */
  bottom: number | null;
  /** Distance from the viewport's right edge; the menu is right-aligned to its button. */
  right: number;
}

/**
 * Where a row's menu goes, in viewport coordinates.
 *
 * The task, interruption and notification lists scroll, so they clip whatever
 * hangs outside them. The menu used to be positioned inside its row: below it,
 * or above it for the last row. With a single task "the last row" is also the
 * first, so its menu opened upwards into the clipped area and Edit and Delete
 * could not be reached at all. Placing it against the window instead means no
 * list can cut it off: below the button when there is room, above otherwise.
 */
export function placeMenu(
  button: Pick<DOMRect, "top" | "bottom" | "right">,
  menuHeight: number,
  viewport: { width: number; height: number },
): MenuPlacement {
  const needed = Math.max(menuHeight, TALLEST_MENU);
  const right = Math.max(MARGIN, viewport.width - button.right);
  const fitsBelow = button.bottom + GAP + needed <= viewport.height - MARGIN;
  const fitsAbove = button.top - GAP - needed >= MARGIN;
  if (fitsBelow || !fitsAbove) return { top: button.bottom + GAP, bottom: null, right };
  return { top: null, bottom: viewport.height - button.top + GAP, right };
}

function position(menu: HTMLDetailsElement) {
  const button = menu.querySelector("summary");
  const popover = menu.querySelector<HTMLElement>(".menu-popover");
  if (!button || !popover) return;
  const placement = placeMenu(button.getBoundingClientRect(), popover.offsetHeight, {
    width: window.innerWidth,
    height: window.innerHeight,
  });
  // On the <details>, which React never restyles, rather than on the popover,
  // which it re-renders when the menu's contents change.
  menu.style.setProperty("--menu-top", placement.top === null ? "auto" : `${placement.top}px`);
  menu.style.setProperty(
    "--menu-bottom",
    placement.bottom === null ? "auto" : `${placement.bottom}px`,
  );
  menu.style.setProperty("--menu-right", `${placement.right}px`);
  menu.dataset.placed = "";
}

function closeAll(except?: Node | null) {
  for (const menu of document.querySelectorAll<HTMLDetailsElement>(`${MENU_SELECTOR}[open]`)) {
    if (!except || !menu.contains(except)) menu.open = false;
  }
}

/**
 * Window-level behaviour for the rows' "more actions" menus, which are plain
 * <details> elements: places each one as it opens, and closes it when the user
 * presses anywhere else or opens another — so only one is ever open — or when
 * what it is attached to moves.
 */
export function useRowMenus() {
  useEffect(() => {
    // `toggle` does not bubble; capturing sees it for every menu.
    const onToggle = (event: Event) => {
      const menu = event.target;
      if (!(menu instanceof HTMLDetailsElement) || !menu.matches(MENU_SELECTOR)) return;
      if (!menu.open) {
        // Placed afresh on every open; until then the stylesheet keeps the
        // popover hidden, so it is never seen where it was last time.
        delete menu.dataset.placed;
        return;
      }
      // A menu opened from the keyboard arrives with no pointer press, so the
      // one before it has to be closed here as well.
      closeAll(menu);
      position(menu);
    };
    const onPointerDown = (event: PointerEvent) =>
      closeAll(event.target instanceof Node ? event.target : null);
    const onMove = () => closeAll();

    document.addEventListener("toggle", onToggle, true);
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("scroll", onMove, true);
    window.addEventListener("resize", onMove);
    return () => {
      document.removeEventListener("toggle", onToggle, true);
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("scroll", onMove, true);
      window.removeEventListener("resize", onMove);
    };
  }, []);
}
