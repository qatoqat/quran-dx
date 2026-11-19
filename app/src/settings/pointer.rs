//! The reading pointer: a dot or ring that takes the mouse cursor's place below
//! the header (the cursor itself is hidden there by the stylesheet).
//!
//! It is moved by a script, so following the mouse costs no round trip through
//! the app; its colour, size, opacity, blur and glow are CSS variables written
//! with the other settings, and only its shape, its trail, whether it glows and
//! how much it shrinks with speed are handed to the script.

use dioxus::prelude::*;

use super::{PointerShape, use_settings};

/// `window.__pointer`, installed once. `set(null)` turns the pointer off;
/// `set({ ring, trail, lag, glow, shrink })` (re)builds it: one dot at the mouse
/// and `trail` fading copies that each chase the one before, closing the share
/// `lag` of the gap every frame (a smaller share leaves a longer trail). With `shrink` (0..=10)
/// the pointer gets smaller the faster the mouse moves — down to a fifth of its
/// size at 10, from about 2000 px/s — and grows back when it slows. For `glow` a lens under the
/// first dot intensifies the colours of the text behind it (the trail has none:
/// a streak of lenses looked wrong). The dots share one opacity (their group's);
/// the lens sits outside that group, because a backdrop filter sees nothing
/// through a translucent ancestor.
const POINTER_JS: &str = r#"
if (!window.__pointer) {
  const layer = document.createElement("div");
  layer.className = "reading-pointer";
  layer.hidden = true;
  document.body.append(layer);
  const group = document.createElement("span");
  let dots = [], lens = null, on = false, shown = false, x = 0, y = 0, frame = 0;
  // Speed in px/ms (smoothed), the size it calls for and the size drawn (1 = full).
  let shrink = 0, speed = 0, size = 1, last = 0, lag = 0.35;
  const draw = () => {
    frame = 0;
    speed *= 0.85;
    size += (1 - shrink * Math.min(1, speed / 2) - size) * 0.3;
    let tx = x, ty = y, moving = speed > 0.01 || Math.abs(1 - size) > 0.01;
    dots.forEach((d, i) => {
      if (i === 0) { d.x = x; d.y = y; }
      else {
        d.x += (tx - d.x) * lag; d.y += (ty - d.y) * lag;
        if (Math.abs(tx - d.x) + Math.abs(ty - d.y) > 0.5) moving = true;
      }
      d.el.style.transform = `translate(${d.x}px, ${d.y}px) scale(${d.scale * size})`;
      tx = d.x; ty = d.y;
    });
    if (lens) lens.style.transform = `translate(${x}px, ${y}px) scale(${size})`;
    if (moving) frame = requestAnimationFrame(draw);
  };
  const show = (visible) => {
    if (visible === shown) return;
    shown = visible;
    layer.hidden = !visible;
    if (visible) for (const d of dots) { d.x = x; d.y = y; }
  };
  addEventListener("mousemove", (e) => {
    if (!on) return;
    if (shrink) {
      const dt = e.timeStamp - last;
      if (dt > 0 && dt < 100) speed = speed * 0.7 + Math.hypot(e.clientX - x, e.clientY - y) / dt * 0.3;
      last = e.timeStamp;
    }
    x = e.clientX; y = e.clientY;
    // The header and the settings panel keep the ordinary cursor.
    show(!e.target.closest?.(".site-header, .settings-panel"));
    if (shown && !frame) frame = requestAnimationFrame(draw);
  }, { passive: true });
  document.documentElement.addEventListener("mouseleave", () => show(false));
  window.__pointer = {
    set(config) {
      on = !!config;
      document.documentElement.classList.toggle("pointer-on", on);
      layer.classList.toggle("ring", !!config?.ring);
      layer.replaceChildren();
      group.replaceChildren();
      dots = [];
      lens = null;
      shrink = (config?.shrink ?? 0) * 0.08;
      lag = config?.lag ?? 0.35;
      speed = 0;
      size = 1;
      shown = false;
      layer.hidden = true;
      if (!on) return;
      if (config.glow) layer.append(lens = document.createElement("b"));
      layer.append(group);
      for (let i = 0; i <= config.trail; i++) {
        const el = document.createElement("i");
        const fade = 1 - i / (config.trail + 1);
        el.style.opacity = String(fade);
        group.append(el);
        dots.push({ el, x, y, scale: 0.4 + 0.6 * fade });
      }
    },
  };
}
"#;

/// Share of the gap a trail copy closes per frame, for a 1..=10 distance:
/// 1 → 0.55 (tight), 5 → 0.35 (default), 10 → 0.1 (long).
fn trail_lag(distance: u8) -> f64 {
    (60.0 - 5.0 * f64::from(distance.clamp(1, 10))) / 100.0
}

/// Keep the pointer in step with the settings. Call once, from the app shell.
pub fn use_reading_pointer() {
    let settings = use_settings();
    let config = use_memo(move || {
        let pointer = &settings.read().pointer;
        pointer.enabled.then(|| {
            let ring = pointer.shape == PointerShape::Ring;
            format!(
                "{{ ring: {ring}, trail: {}, lag: {}, glow: {}, shrink: {} }}",
                pointer.trail.min(10),
                trail_lag(pointer.trail_distance),
                pointer.glow > 0,
                pointer.shrink.min(10)
            )
        })
    });
    use_effect(move || {
        let config = config().unwrap_or_else(|| "null".into());
        document::eval(&format!("{POINTER_JS}\nwindow.__pointer.set({config});"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longer_distance_means_slower_catch_up() {
        assert_eq!((trail_lag(1), trail_lag(5), trail_lag(10)), (0.55, 0.35, 0.1));
        assert_eq!((trail_lag(0), trail_lag(99)), (0.55, 0.1));
    }
}
