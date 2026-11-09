# canvas

![A character sheet over a 3D world of stone pillars: a framed portrait of the hero (a capsule with a head, in leather armor, turned to the side), a small thumbnail of the same render, the hero's name and armor, buttons to change the armor, hide the portrait and switch the language, and a status line reading "The portrait camera renders."](../screenshots/canvas.png)

`<canvas>` (ADR 0041): a live 3D render inside an HTML template.

```sh
cargo run --example canvas
```

## What you see

A character sheet over a small world of pillars. The portrait is a live
render of the hero, who exists only on render layer 1: the portrait camera
draws that layer into an image, the world camera draws layer 0. The hero
stands at the origin, between the pillars, and never shows up there.

| Input | Action |
|---|---|
| Drag over the portrait | Turn the hero (Bevy's mesh picking, through the canvas) |
| Click the hero | Change the armor |
| Q / E, LB / RB | Turn the hero |
| ↑ ↓ / D-pad / left stick | Move between the buttons |
| Enter / gamepad A | Press the focused button |
| Space / gamepad Y | Switch the language (English ↔ German) |

## What it shows

- **The template names, the app binds**: `<canvas data-canvas="portrait"
  width="240" height="320">` sizes the box; the app spawns the camera
  (`RenderTarget::Image`) and registers it with
  `HtmlCanvases::insert("portrait", CanvasSource::Camera(camera))`.
- **Bevy's `ViewportNode` does the rest**: it resizes the camera's image to
  the box (at the display's scale factor) and forwards the pointer into the
  camera's scene, where `MeshPickingPlugin` hits the hero.
- **One viewport per camera**: the thumbnail names the same camera, so it
  shows the camera's image, stretched, instead of a second viewport.
- **Turning the camera off**: "Hide the portrait" hides both canvases
  (CSS `display: none`); `HtmlCanvases::shown("portrait")` turns false and
  the app sets `Camera::is_active` from it (the status line follows).
- **The canvas is a box**: its border, radius and background come from CSS;
  the camera clears to transparent, so the background shows behind the
  hero.

## Files

| File | Contents |
|---|---|
| `main.rs` | The world, the hero on its render layer, the portrait camera and its registration, input |
| `../assets/canvas/sheet.html` | The template (two canvases naming one camera) |
| `../assets/canvas/style.css` | The stylesheet |
| `../assets/canvas/locales/{en-US,de}/` | Fluent bundles |
