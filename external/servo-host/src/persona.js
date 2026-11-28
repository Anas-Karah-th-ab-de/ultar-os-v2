// Ultar Browser – the persona every page sees (apps/ultar-web/src/privacy/persona.rs).
// Runs before the page's own scripts in every document. The values are filled in by the host
// (persona.rs); the same for every user, nothing of this device.
(() => {
  "use strict";
  const P = __PERSONA__;
  const fix = (target, name, value) => {
    try {
      Object.defineProperty(target, name, { get: () => value, configurable: false, enumerable: true });
    } catch (_) {}
  };
  // navigator: the crowd of Tor Browser on Windows
  const nav = Navigator.prototype;
  fix(nav, "platform", P.platform);
  fix(nav, "oscpu", P.oscpu);
  fix(nav, "hardwareConcurrency", P.cores);
  fix(nav, "language", P.language);
  fix(nav, "languages", Object.freeze(P.languages.slice()));
  fix(nav, "webdriver", false);
  fix(nav, "maxTouchPoints", 0);
  fix(nav, "doNotTrack", "unspecified");
  // the screen is the persona's, not this device's
  const scr = Screen.prototype;
  for (const [name, value] of [["width", P.screen[0]], ["height", P.screen[1]],
      ["availWidth", P.screen[0]], ["availHeight", P.screen[1]], ["colorDepth", 24], ["pixelDepth", 24]]) {
    fix(scr, name, value);
  }
  fix(window, "devicePixelRatio", 1);
  // coarse clocks (as Tor Browser): no timing attacks on the device
  const step = P.clockStepMs;
  const coarse = (t) => Math.floor(t / step) * step;
  const now = Performance.prototype.now;
  Performance.prototype.now = function () { return coarse(now.call(this)); };
  // canvas pictures cannot be read back (a canvas fingerprints the device's fonts and GPU):
  // reading gives a blank picture of the same size, drawing still works
  const blank = (w, h) => {
    const c = document.createElement("canvas");
    c.width = w; c.height = h;
    return c;
  };
  const toDataURL = HTMLCanvasElement.prototype.toDataURL;
  HTMLCanvasElement.prototype.toDataURL = function (...a) {
    return toDataURL.apply(blank(this.width, this.height), a);
  };
  if (HTMLCanvasElement.prototype.toBlob) {
    const toBlob = HTMLCanvasElement.prototype.toBlob;
    HTMLCanvasElement.prototype.toBlob = function (...a) {
      return toBlob.apply(blank(this.width, this.height), a);
    };
  }
  if (window.CanvasRenderingContext2D) {
    CanvasRenderingContext2D.prototype.getImageData = function (x, y, w, h) {
      const data = new ImageData(Math.max(1, Math.abs(w | 0)), Math.max(1, Math.abs(h | 0)));
      data.data.fill(255);
      return data;
    };
  }
})();
