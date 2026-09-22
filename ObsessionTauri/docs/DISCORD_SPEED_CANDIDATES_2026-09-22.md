# Discord14 speed candidates

The manual configuration menu remains available. These are optional experiments,
not a replacement for the user's working selection and not measured speed gains.

| Profile | TCP fake repeats | Status |
| --- | --- | --- |
| discord_14.conf | 8 | Original; user confirmed connectivity |
| discord_15.conf | 4 | Experimental copy |
| discord_16.conf | 2 | Experimental copy |

Only `--dpi-desync-repeats` in the two TCP profiles differs. QUIC stays at 11,
voice UDP at 6; host scope, timestamp fooling, payloads and split position are
unchanged. A structural regression test checks these exact differences.

Keep YouTube11 unchanged. Fewer fake packets do not necessarily improve speed:
they can also reduce bypass reliability. Compare repeated downloads of the same
Discord CDN assets, plus gateway/updater and voice checks, before promoting either
candidate. The small default avatar is useful for connectivity, not a throughput
benchmark. The supplied c7.patreon.com URL is outside the Discord host list and
cannot be used directly to rank these profiles.

Provenance of the existing Discord set: 1–10 are scoped Flowseal 1.10.3 adaptations;
11–12 are Obsession compatibility candidates; 13 is adapted from ImMALWARE;
14 is adapted from kinlay0's EXPERIMENTAL TS strategy. See individual headers.

Baseline SHA-256:
- Discord14: 9dbb17e46420d530e9e257e9a935d191c2269a9ac29ec177b9d6017bbcc39eef
- YouTube11: 96396cfa07191e5ff31c4f83c932366b8d448a7c65e858000f2307da8f55eef7

No live runtime switching or installation was performed while preparing this
package. Live A/B performance validation remains outstanding.
