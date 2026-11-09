import "./styles.css";

// The clips loop on their own; with Reduce Motion they stay on their poster.
if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
  for (const v of document.querySelectorAll("video[autoplay]")) {
    v.removeAttribute("autoplay");
    v.pause();
  }
}
