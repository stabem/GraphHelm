# Stalled card: reason, unblocker and place (#2531)

Static before/after mock of a stalled card on the Studio's Graph and Lanes views.

- `before.html` / `before.png`: today. "Stalled · owes a record" with no reason, unblocker or place.
- `after.html` / `after.png`: proposed. A stalled card names the reason ("no reason given (silent since 14:05)" when none is declared), the step and last record, and the unblocker (a lane name, never a bare "owner") with a link to the place.

Owner approval: Gabriel (owner), 2026-11-07, via the OpenBot confirm card relayed by Shikamaru. Approval comment: https://github.com/stabem/GraphHelm/issues/2531#issuecomment-6711789622.

Code waits on #2524's confirmed fields.
