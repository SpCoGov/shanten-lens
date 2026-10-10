(function () {
  var THEME_KEY = "sl-theme";
  var root = document.documentElement;
  var saved = null;
  var custom = null;
  try {
    saved = localStorage.getItem(THEME_KEY);
    if (saved === "custom") custom = JSON.parse(localStorage.getItem("sl-custom-theme") || "null");
  } catch (_) {
  }

  var next = saved;
  if (saved === "custom") {
    var base = custom && custom.base === "light" ? "light" : "dark";
    root.setAttribute("data-custom-theme", base);
    next = base === "dark" ? "dark" : "";
    var keys = ["--color-bg", "--panel-bg", "--color-text", "--color-muted", "--color-border",
      "--color-ring", "--input-bg", "--state-ok", "--state-warn", "--state-down"];
    keys.forEach(function (key) {
      var color = custom && custom.colors && custom.colors[key];
      if (typeof color === "string" && /^#[\da-f]{6}$/i.test(color)) root.style.setProperty(key, color);
    });
  }
  if (saved !== "custom" && next !== "dark" && next !== "dark-green" && next !== "dark-purple") {
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
