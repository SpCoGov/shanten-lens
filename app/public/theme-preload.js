(function () {
  var THEME_KEY = "sl-theme";
  var root = document.documentElement;
  var saved = null;
  try {
    saved = localStorage.getItem(THEME_KEY);
  } catch (_) {
    saved = null;
  }

  var next = saved;
  if (!next || next === "auto") {
    var prefersDark = false;
    try {
      prefersDark = !!(window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches);
    } catch (_) {
      prefersDark = false;
    }
    next = prefersDark ? "dark" : "";
  }

  if (next === "dark" || next === "dark-green" || next === "dark-purple") {
    root.setAttribute("data-theme", next);
  } else {
    root.removeAttribute("data-theme");
  }
})();
