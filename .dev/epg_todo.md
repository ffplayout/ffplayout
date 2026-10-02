# XMLTV EPG export backlog

This is a possible read-only export of ffplayout's planned schedule, not a
commitment to build a metadata catalogue or a guarantee of what is currently
on air. There is no implementation timeline; see `.dev/README.md`.

## Scope and decisions

- [ ] Confirm whether a public XMLTV endpoint should be opt-in, and define a
  stable channel ID and a bounded export window (for example, a few days).
- [ ] Keep the authenticated JSON programme API and playlist files unchanged.
  Share schedule calculation where practical, but do not expose media paths,
  internal configuration, or credentials in the public response.
- [ ] Treat TMDB or other external metadata providers as separate, optional
  work. XMLTV must work without an API key or network metadata lookup. Do not
  infer a movie identity solely from a filename.

## Schedule and metadata

- [ ] Export channel ID/name and, for each scheduled item, XMLTV `start`,
  `stop`, `channel`, and `<title>`. Use the optional playlist title where
  available; define a safe, human-readable filename fallback that never
  reveals a full source path or URL.
- [ ] Decide whether to add an optional playlist `description` field. Emit
  `<desc>` only when a description is available; keep older playlists valid.
  Genre, artwork, and extended metadata are not required for the first version.
- [ ] Preserve exact calculated boundaries, including seconds and timezone
  offsets. Do not round each clip boundary to a minute or create artificial
  gaps/overlaps. Test day changes, DST transitions, non-midnight day starts,
  trimmed clips, and multi-day playlists.
- [ ] Define advertising behavior explicitly. Existing `ad` items may be
  exported as a neutral "Advertisement" entry to preserve the timeline;
  hiding or grouping them with a programme requires reliable programme
  grouping data and must not silently change adjacent programme times.
- [ ] Document that live takeover, missing media, fallback, and later playlist
  edits can make the actual output differ from the published schedule.

## Endpoint and caching

- [ ] Produce valid, escaped XMLTV from a bounded, read-only HTTP endpoint;
  decide the URL and public-access policy before implementation. Reject
  excessive date ranges instead of scanning an unbounded playlist history.
- [ ] Cache parsed schedules or rendered XMLTV per channel and export window
  in memory so repeated public requests do not reread every playlist file (or
  refetch remote playlists) and regenerate XML on every request.
- [ ] Bound cache entries and lifetime. Invalidate affected entries after
  playlist save, generate, import, or delete, and after relevant channel or
  playlist settings change. Account for external file edits and remote
  playlists with a watcher or a short TTL; avoid a filesystem stat for every
  cached request if that would defeat the I/O reduction.
- [ ] Consider ETag/Last-Modified and a suitable HTTP Cache-Control policy so
  clients and reverse proxies can avoid downloading unchanged feeds. Never
  serve an indefinitely stale schedule after an edit.

## Verification

- [ ] Validate generated XML against the XMLTV DTD and test XML escaping,
  missing titles/descriptions, advertising, timezone boundaries, and empty or
  missing playlists.
- [ ] Test cache hits, invalidation, expiry, concurrent requests, and bounded
  memory use. Verify that repeated requests do not repeat playlist reads.
- [ ] Add API tests for public-access policy, range limits, content type, and
  omission of private paths/URLs; document the difference between planned and
  actual playout.

**Acceptance criteria:** an XMLTV consumer can read a bounded, current planned
schedule without authentication only when public access is enabled; new or
edited playlists become visible within a defined freshness window; ordinary
requests are served from cache without repeated playlist I/O; the feed works
without TMDB and does not leak private media paths.
