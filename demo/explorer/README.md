# riffcat twin explorer

A rough-on-purpose, no-build-step page over `riffcat overlap --json`: pick a
contract pair, drag the facet dial, watch twin classes and the Jaccard readout
move. Vanilla JS web components, one stylesheet, works from `file://` offline.

```
./generate-data.sh   # after demo/stage.sh; rewrites data.js from the corpus
open index.html      # or just double-click it
```

Deliberately unfinished: the page consumes the CLI's JSON verbatim — including
its quirks (member lists capped with `(+N)`, summary counts absent from the
JSON and scraped from text output instead). Those quirks ARE the agenda for the
co-design conversation: what should a "similar contracts" payload actually
carry? Don't polish this page before that conversation happens.

`data.js` is generated (committed so the page works on a fresh clone without
re-staging); regenerate after any corpus change.
