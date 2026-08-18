'use strict';
// JII — the win marker for the Omega Flowey fight.
//
// Everything JII adds to this game lives here. `electron-main.js` calls `watch()` once, on the
// window it just opened; nothing else in the project is touched.
//
// The fight's own win condition is a stage variable: the project sets `flowey hp` to 9950 when
// the battle starts and broadcasts `flowey death` the moment it drops below 1. We watch the same
// variable from the main process (the page itself stays sandboxed and untouched) and write a
// one-line marker next to JII's state when it happens. JII reads that marker on its next run.
//
// The marker records *which* fight was won — `normal` or `hard` — from the game's own
// `hard mode?` menu toggle.
//
// Two environment variables, both for testing: JII_FLOWEY_MARKER overrides where the marker is
// written, JII_FLOWEY_DEBUG prints every poll to stdout.
const fs = require('fs');
const os = require('os');
const path = require('path');

const MARKER_NAME = 'flowey-install';
const POLL_MS = 500;

// Read in the page's own world. Returns null until the VM is up, so a slow load is just a wait.
const PROBE = `(() => {
  try {
    const stage = window.vm && window.vm.runtime.getTargetForStage();
    if (!stage) return null;
    const hp = stage.lookupVariableByNameAndType('flowey hp', '');
    if (!hp) return null;
    const hard = stage.lookupVariableByNameAndType('hard mode?', '');
    return [Number(hp.value), hard ? String(hard.value) : 'off'];
  } catch (e) {
    return null;
  }
})()`;

// The fight reads letter keys (`z` to confirm, `x`/`c` in dialogue) — and Scratch matches on the
// *character* a key produces, not on the key itself. On a non-Latin layout (Cyrillic, Greek…)
// `z` arrives as `я` and the game sees nothing: the menu moves on the arrows but nothing ever
// confirms. Rather than tell the player to go change their layout, feed the runtime the Latin
// letter the physical key stands for, alongside whatever the layout produced.
const LAYOUT_FIX = `(() => {
  if (window.__jiiLayoutFix) return 'already';
  const latin = (event) => {
    const match = /^Key([A-Z])$/.exec(event.code || '');
    return match ? match[1].toLowerCase() : null;
  };
  const forward = (isDown) => (event) => {
    const key = latin(event);
    // Nothing to do when the layout already produced that very letter.
    if (!key || (typeof event.key === 'string' && event.key.toLowerCase() === key)) return;
    try {
      window.vm.runtime.ioDevices.keyboard.postData({ key: key, isDown: isDown });
    } catch (e) {
      // No VM yet, or a runtime that doesn't take keys — the layout stays the player's problem.
    }
  };
  document.addEventListener('keydown', forward(true), true);
  document.addEventListener('keyup', forward(false), true);
  window.__jiiLayoutFix = true;
  return 'installed';
})()`;

const markerPath = () => {
  if (process.env.JII_FLOWEY_MARKER) return process.env.JII_FLOWEY_MARKER;
  const state = process.env.XDG_STATE_HOME || path.join(os.homedir(), '.local', 'state');
  return path.join(state, 'jii', MARKER_NAME);
};

const writeMarker = (variant) => {
  try {
    const file = markerPath();
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, variant + '\n');
  } catch (e) {
    // A marker we can't write costs an achievement, not a game. Stay quiet.
  }
};

// Arm only after seeing a health bar above zero: a project that hasn't started yet must never
// look like a win, however it was launched.
const watch = (webContents) => {
  // Re-applied on every load, so a reload doesn't lose it.
  const installLayoutFix = () => {
    webContents.executeJavaScript(LAYOUT_FIX, true).then((result) => {
      if (process.env.JII_FLOWEY_DEBUG) console.log('jii: layout fix ->', result);
    }).catch(() => {});
  };
  webContents.on('dom-ready', installLayoutFix);
  installLayoutFix();

  let armed = false;
  const timer = setInterval(() => {
    if (webContents.isDestroyed()) {
      clearInterval(timer);
      return;
    }
    webContents.executeJavaScript(PROBE, true).then((result) => {
      if (process.env.JII_FLOWEY_DEBUG) console.log('jii: probe ->', JSON.stringify(result));
      if (!Array.isArray(result)) return;
      const hp = Number(result[0]);
      if (!Number.isFinite(hp)) return;
      if (!armed) {
        armed = hp >= 1;
        return;
      }
      if (hp < 1) {
        clearInterval(timer);
        const variant = result[1] === 'on' ? 'hard' : 'normal';
        if (process.env.JII_FLOWEY_DEBUG) console.log('jii: win ->', variant, markerPath());
        writeMarker(variant);
      }
    }).catch(() => {});
  }, POLL_MS);
};

module.exports = { watch };
