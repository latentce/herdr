# Animated status indicators

Fork-only feature. Stock Herdr's documentation does not describe it.

The `animated` agent status style sits alongside upstream's `dots` and `symbols`:

| State   | `dots` | `symbols` | `animated`                                   |
| ------- | ------ | --------- | -------------------------------------------- |
| Working | ●      | ◐         | braille spinner `⠒⠲⠰⠴⠤⠦⠆⠖`, 100 ms per frame |
| Blocked | ●      | ×         | ● blinking                                   |
| Done    | ●      | ✓         | ● blinking                                   |
| Idle    | ○      | ○         | ○                                            |
| Unknown | ·      | ·         | ·                                            |

The spinner is a trail circling the middle 2×2 dots of the braille cell. It turns once every 800 ms, about as fast as the common terminal spinners.

Every state keeps its usual color. Blocked and Done blink continuously: 500 ms at full color, then 500 ms faded halfway into the background behind the icon, so they never disappear and fade the same way on dark and light themes. When an icon sits on the terminal's own background and the terminal doesn't report that color, or the theme uses named terminal colors, the faded half uses the terminal's faint text instead. All animated icons share one clock, so every spinner shows the same frame and every blinking icon is in the same phase.

The style applies everywhere `dots` and `symbols` do: the sidebar's workspace and agent rows, collapsed folders, the collapsed sidebar, other machines' rows, and the mobile header, summary and switcher. In the mobile summary, idle agents stay unmarked as with `dots`. Rows of a machine that is offline are dimmed and keep a still ●, since their last known state may be stale.

## Choosing it

Open **settings** from the menu, on desktop or in the mobile view, go to the indicators section and pick **animated dots**. Picking **color dots** or **distinct symbols** turns the animation off.

In config, use the fork-only key:

```toml
[ui]
status_indicator_style = "animated"
```

`status_indicator_style` accepts `"dots"`, `"symbols"` or `"animated"`. When set, it replaces `status_indicators`; it is unset by default. Choosing dots or symbols in settings writes `status_indicators` and removes `status_indicator_style`; choosing animated writes only `status_indicator_style`.

Do not set `status_indicators = "animated"`. Stock Herdr rejects unknown values there and falls back to default settings for the entire config, so this build rejects it too, while stock Herdr only warns about the unknown `status_indicator_style` key and keeps everything else.

## Cost

With `dots` or `symbols`, nothing changes: no timers and no extra redraws. With `animated`, Herdr redraws only while an animated icon is visible, and only when one of them changes: every 100 ms while a spinner is visible, every 500 ms while only blocked or done icons are. The blink always flips on a spinner frame, so blinking adds no redraws while a spinner is visible. Idle and unknown agents, rows scrolled out of view, dimmed offline rows, a hidden sidebar and icons covered by a dialog schedule nothing.

Each animation step is an ordinary redraw that sends only the changed cells to the terminal. Drawing the animated icons adds up to about 25 µs to a redraw that takes 0.6–0.8 ms at 120×40 in a release build, and that overhead stays flat from 1 to 50 panes. While a spinner is visible, redrawing every 100 ms takes under 1% of one CPU core at that size, a little more on larger screens.

## Compatibility with stock Herdr

- The animation is drawn by the client, so it works against servers and machines running stock Herdr.
- Stock Herdr reading the same config shows the style from `status_indicators` and only warns about the unknown `status_indicator_style` key.
