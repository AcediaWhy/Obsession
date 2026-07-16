# Screen Transition Liveness Design

## Problem

After leaving the DPI screen, the selected navigation item changes but the main
content area can remain empty. The top-level screen transition uses
`AnimatePresence` in `wait` mode. That mode withholds the entering screen until
every exit participant in the outgoing DPI subtree reports completion. The DPI
screen now contains several nested animated regions, so an interrupted or stale
exit can leave the navigation shell waiting indefinitely.

## Scope

The fix is limited to the screen transition boundary in `src/App.tsx`. It must
not change the navigation, page layouts, themes, HeroCore, or the contents of
the DPI and other screens.

## Design

The outgoing and entering screen wrappers render synchronously in a shared
positioned container. Both wrappers occupy the same content bounds, so the
entering screen cannot be pushed outside the viewport while the outgoing screen
finishes. The existing directional slide, spring, and descendant fade
animations remain unchanged.

The entering wrapper is rendered after the outgoing wrapper and therefore stays
visible and interactive even if cleanup of the outgoing subtree is delayed.
This removes the liveness dependency on nested DPI exit animations without
removing the established transition design.

## Verification

- TypeScript compilation and the production frontend build pass.
- Starting from DPI, every navigation destination renders content.
- Returning to DPI still renders its complete interface.
- Repeated and rapid navigation does not leave the content area empty.
- Existing screen slide direction and panel fades remain visible.
