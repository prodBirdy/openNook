import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

const root = document.getElementById("root") as HTMLElement;
// Add dark class to html or body to ensure tailwind dark: variants work
document.documentElement.classList.add('dark');

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
