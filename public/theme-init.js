// Runs before the first paint: a classic script in <head> blocks rendering,
// where the app's module script is deferred and the stylesheet can paint a
// frame first. Without it, someone who chose Dark on a light desktop (or the
// reverse) sees one frame of the wrong theme on every launch.
//
// A file rather than an inline script because the content security policy
// only allows scripts from the app itself. The key is written by applyTheme in
// src/lib/theme.ts; theme.test.tsx runs this file against it so the two cannot
// drift apart. Anything unreadable leaves "system", which the stylesheet
// resolves from the desktop by itself.
(function () {
  try {
    var stored = window.localStorage.getItem("pomodoro.theme");
    if (stored === "light" || stored === "dark" || stored === "system") {
      document.documentElement.dataset.theme = stored;
    }
  } catch (error) {
    // Storage is unavailable; the markup's data-theme="system" stands.
  }
})();
