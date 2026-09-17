# Rain cat: source-registered layers v1

Prepared locally from the user-supplied original with Pillow/numpy, without image generation. `prepare_layers.py` reproduces the exports. The older `PREPARATION.md` documents the rejected generation attempt, not the status of this set.

## Files

- `layers-v1/`: six registered RGBA PNG masters and lossless WebP equivalents, 1254×1254.
- `layers-v1/256/`: six lossless WebP UI-size candidates; common 256×256 canvas.
- `layers-v1/manifest.json`: stacking order, master-coordinate pivots, bounding boxes and file sizes. Scale coordinates by 256/1254 for the small version.
- `layers-v1/assembled.png`: static recomposition on transparency.
- `review-v1.jpg`: recomposition on white and dark blue backgrounds.
- `layers-contact-sheet.jpg`: separate layers for inspection.

Bottom-to-top: puddle → umbrella → tail → cat-body → holding-paw → eyes.

## Preserved and reconstructed areas

Visible artwork is cut directly from the original; the white paper matte is removed. Eye whites and inner fur highlights are retained. Eyes are replaced with sampled fur on the body layer. Yellow contamination from the canopy is excluded at the ears.

The canopy behind the cat is patched using original underside texture; the hidden shaft is extended. The tail has a hidden black root overlap. A small puddle patch fills the region previously covered by the feet. These reconstructed areas are an approximation for modest idle motion, not artwork for large turns or exposing the entire hidden canopy.

The head remains joined to the torso. The small holding paw is separate. Keep the umbrella shaft registered to the paw pivot when rigging. Tail motion should bend away from its pinned root rather than rotate the whole cutout.

Rain/application code and assets have not been replaced. Animation extremes and the final small-size appearance still need validation during rigging.
