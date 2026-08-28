"use strict";
(() => {
  // src/index.tsx
  var api = window.__openNookPluginAPI__;
  var {
    registerWidget,
    React,
    WidgetWrapper,
    icons
  } = api;
  var { IconBox } = icons;
  var { useState, useEffect } = React;
  var STORAGE_KEY = "external-counter-value";
  function ExternalCounterWidget() {
    const [count, setCount] = useState(() => {
      const saved = localStorage.getItem(STORAGE_KEY);
      return saved ? parseInt(saved, 10) : 0;
    });
    useEffect(() => {
      localStorage.setItem(STORAGE_KEY, count.toString());
    }, [count]);
    return /* @__PURE__ */ React.createElement(WidgetWrapper, { title: "Counter" }, /* @__PURE__ */ React.createElement("div", { className: "flex h-full flex-col items-center justify-center gap-4 py-4" }, /* @__PURE__ */ React.createElement("div", { className: "text-5xl font-bold text-white tabular-nums" }, count), /* @__PURE__ */ React.createElement("div", { className: "flex gap-3" }, /* @__PURE__ */ React.createElement(
      "button",
      {
        onClick: (e) => {
          e.stopPropagation();
          setCount((c) => c - 1);
        },
        className: "flex h-10 w-10 items-center justify-center rounded-full bg-white/10 text-white text-xl hover:bg-white/20 transition-colors"
      },
      "\u2212"
    ), /* @__PURE__ */ React.createElement(
      "button",
      {
        onClick: (e) => {
          e.stopPropagation();
          setCount(0);
        },
        className: "flex h-10 w-10 items-center justify-center rounded-full bg-white/5 text-white/60 text-sm hover:bg-white/10 hover:text-white transition-colors"
      },
      "\u21BA"
    ), /* @__PURE__ */ React.createElement(
      "button",
      {
        onClick: (e) => {
          e.stopPropagation();
          setCount((c) => c + 1);
        },
        className: "flex h-10 w-10 items-center justify-center rounded-full bg-blue-500 text-white text-xl hover:bg-blue-400 transition-colors"
      },
      "+"
    ))));
  }
  function CompactExternalCounter({ contentOpacity }) {
    const [count, setCount] = useState(() => {
      const saved = localStorage.getItem(STORAGE_KEY);
      return saved ? parseInt(saved, 10) : 0;
    });
    useEffect(() => {
      const interval = setInterval(() => {
        const saved = localStorage.getItem(STORAGE_KEY);
        if (saved)
          setCount(parseInt(saved, 10));
      }, 500);
      return () => clearInterval(interval);
    }, []);
    return /* @__PURE__ */ React.createElement(
      "div",
      {
        style: { opacity: contentOpacity },
        className: "flex items-center gap-1 text-xs text-white/80"
      },
      /* @__PURE__ */ React.createElement(IconBox, { size: 14 }),
      /* @__PURE__ */ React.createElement("span", { className: "tabular-nums font-medium" }, count)
    );
  }
  registerWidget({
    id: "external-counter",
    name: "External Counter",
    description: "Example external plugin - a simple counter",
    icon: IconBox,
    ExpandedComponent: ExternalCounterWidget,
    CompactComponent: CompactExternalCounter,
    defaultEnabled: false,
    category: "utility",
    minWidth: 200,
    hasCompactMode: true,
    compactPriority: 100
  });
  console.log("\u2705 External Counter plugin registered");
})();
