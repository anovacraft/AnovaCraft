# Automated GUI tests

The application's screens are tested automatically, without a window or a
graphics card, on every pull request. Each test starts the whole app,
clicks menus, buttons and drop-down lists, types into fields and drags
axes the way a person would, then checks what is on screen and what the app
did. A change that breaks a dialog or a plot interaction fails CI before it
reaches anyone.

## Running them

The GUI tests run with the rest of the test suite:

```sh
cargo test --workspace --no-fail-fast
```

To run only the GUI tests:

```sh
cargo test -p app-gui gui_tests
```

## Pictures of the screens

Set `ANOVACRAFT_GUI_SHOTS` to a folder to also save a picture of the window
at each point a test marks:

```sh
ANOVACRAFT_GUI_SHOTS=gui-shots cargo test -p app-gui gui_tests
```

Rendering the pictures needs a graphics driver: a Mac uses its own (Metal),
and on a Linux machine without a graphics card, Mesa's software Vulkan
driver (`mesa-vulkan-drivers` on Debian and Ubuntu) works. The pictures
are a record for reviewers, not a comparison: a test fails only on what it checks, never on
a pixel difference, so fonts or graphics drivers that draw slightly
differently on another computer do not fail it.

For design reviews, `feedback_tour` (in `gui_tests/tour.rs`, run on demand)
saves 60 numbered pictures of the most used screens: the welcome page and
data grid, every top menu open, the common dialogs, a report from each main
sample, the graph canvas, scripts and settings, and a few dark-theme views:

```sh
ANOVACRAFT_GUI_SHOTS=feedback-shots cargo test -p app-gui feedback_tour -- --ignored
```

## Where they live

The tests are in `crates/app-gui/src/app/gui_tests/`, grouped by area
(dialogs, documents, plots, scripts). They start the app with the default
settings, so the settings saved on your computer never change what they
see, and they never write to it.

Each test finds controls by the text a screen reader would read: a button's
label, a window's title, a check box's text. Controls drawn without text,
such as the axis rulers, are given an accessible name so tests (and screen
readers) can find them.

## Adding tests

A pull request that adds or changes a dialog, a report interaction or a
plot gesture adds a GUI test for it, and a fix for a GUI bug adds a test
that fails without the fix.
