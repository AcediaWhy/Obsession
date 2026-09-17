# Golden meadow study

Open `/golden-meadow-lab.html` in the existing Vite development server.

The lab opens with an optional window-layout mock over the scene. It follows
the application's 220px navigation rail, 40px title bar and 24px content inset,
with Overview and Settings samples at 1000x680 or 800x600 logical pixels.
The entire mock scales to fit the preview while the canvas uses undistorted
cover sizing. Panel density is adjustable; the controls and statuses inside
the mock are examples, not live application actions or an exact screen replica.
Hide the panels to return to the unobstructed illustration. Panels intercept
pointer input, preserving the existing visible-head-only petting behavior.

Reference: `jeeklaart.jpg`, supplied by the user; the local copy is used only by
the optional side-by-side comparison. The illustration is not sampled by the
Canvas renderer. This lab is separate from the main application entry point.

The scene is drawn by `src/labs/goldenMeadowScene.js`: seeded dry-brush marks,
cached paper grain, three pumpkin silhouettes with individual scale/rotation,
and a cat whose body extends behind an opaque, irregular grass bank. Dense blades
cover the bank's edge. No blurred field overlay or image displacement is used.

Broad diagonal canopy shadows and warm sunlight breaks give the distant field
larger tonal shapes. Two cached, grain-edged masks use multiply and screen over
the distant grass and pumpkins, before the cat and foreground bank. The clearing
around the cat stays illuminated. Lighting is static, spatially deterministic,
and preserves the underlying blade texture without blur or per-frame noise.

A tapered autumn branch enters from the upper-right corner, framing the field
with ochre maple-like and rounded lobed leaves. The woody silhouette and leaf
pigment are cached. Leaves turn slightly about their attached petioles using
the existing wind clock; the branch leaves the central cat silhouette clear.

Pumpkins use broad, unequal lobes, subtly uneven contours, short bent stems,
and matte orange pigment. Distant and foreground specimens have individually
adjusted proportions based on the reference, including the smaller ivory fruit.

Foreground depth comes from broad ochre reed leaves, textured spindle-shaped
seed heads, and warm shadows beneath the illuminated grass tufts and pumpkins.
The tall stalks are cached as part of the drawing. Rooted blades across the
middle distance and grass bank bend under travelling, spatially delayed gusts.
Static paint is grouped into shallow depth bands, split at every pumpkin and
resting leaf so moving grass behind them cannot cross their upper silhouettes.

The middle distance uses a continuous distribution of grass tufts, with density
and height varying across the field. Pumpkins and vegetation are painted in
ground-depth order, replacing the isolated pale grass pads around each group.
Fallen leaves in several silhouettes appear at different depths; foreground
grass partly crosses the resting leaves. Branched dry plants were removed after
visual review; the tall seed heads remain the main accents among the grass.

Animation: fixed-length tail segments, eyelid curves, independent brief ear
turns with long rests, rooted grass blades, and up to two small drifting leaves
behind the cat. Ear control points move within a single closed head outline;
the roots, cheeks and eyes remain fixed. Textures are seeded once and remain
stable between frames. Ear turns ease in over half a second and return slowly;
the left ear has a smaller angle and no repeated automatic twitch.
Pause, manual blink/tail/ear gestures, wind strength and
reference comparison are exposed in the lab. A separate wind clock freezes
grass and airborne leaves at zero wind without stopping the cat. Playback
pauses when the page is hidden and respects the initial reduced-motion preference.

Petting: a short cursor stroke across the visible forehead/cheeks produces a
slow 2.6-second squint and a small tail response. Pointer coordinates map from
the current canvas rectangle into scene space; the topmost element must be the
canvas, so overlaid controls cannot trigger petting. Entry, a stationary pointer,
fast jumps and strokes outside the head do not count. Responses have a cooldown
and never resume a paused scene. Focus the drawing and press Enter or Space for
the keyboard equivalent. This interaction is currently available in the lab.

Whole-preview interaction also listens above the mock panels without consuming
input. Resuming cursor movement after 2.2 seconds of rest elicits a delayed turn
of the nearer ear and a small tail response, at most once per nine seconds.
Broad sweeps send a directional 2.4-second gust across the grass and framing
branch, with spatial delay and a cooldown. The cursor gust works independently
of ambient wind strength. Both responses use playback time and respect pause.

The main app now uses the same scene as its Golden Meadow theme, replacing the
QuietPond theme slot. Existing stored QuietPond selections resolve to Golden
Meadow. The app observes cursor movement over real panels while leaving panel
input intact; the lab keeps its separate controls and layout mock. The main
app uses its shared render scheduler and pauses the scene in a hidden window or
under reduced motion.

This remains a procedural interpretation: shapes and brushwork are not a
pixel-perfect reproduction of the reference.
