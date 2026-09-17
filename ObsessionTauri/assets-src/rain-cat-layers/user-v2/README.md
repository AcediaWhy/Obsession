# User-supplied Rain cat layers

All six supplied files are copied byte-for-byte into `originals/`. They contain real alpha transparency. The umbrella is 1536×1024; the other images are 1254×1254. Matching canvas dimensions alone did not guarantee registered object scale: the eyes also needed adjustment to fit the face.

`assemble.py` creates a static registration draft. It preserves supplied RGB/alpha content, apart from ordinary uniform resampling of umbrella and eyes. It does not remove backgrounds, regenerate artwork, change colors or erase highlights. `registration-draft.json` records all transforms.

View `review-draft.jpg` for white/dark background comparison and `assembly-draft.png` for the transparent composite. This is a visual approval draft, not a finished animation rig. Body/head art was supplied without eyes; eyes, paw and tail are independent layers. No Rain application files have been changed.

Previous automatically cut/generated versions outside this directory are not inputs to this assembly.
