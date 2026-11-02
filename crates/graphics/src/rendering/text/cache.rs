//! Bounded glyph-cache eviction; rasterization owns the cache and calls this after draws.
use std::mem;

use cosmic_text::SwashCache;

const MAX_GLYPH_CACHE_ENTRIES: usize = 4096;
const MAX_GLYPH_CACHE_BYTES: usize = 32 * 1024 * 1024;

// Swash caches are append-only while drawing. Unchanged entry counts need no
// repeated byte scan. Evict the largest entries first with 25% headroom, keeping
// small reusable glyphs instead of forcing a complete cache rebuild.
pub(super) fn trim_glyph_cache(cache: &mut SwashCache, checked: &mut Option<(usize, usize)>) {
    let counts = (cache.image_cache.len(), cache.outline_command_cache.len());

    if *checked == Some(counts) {
        return;
    }

    let mut entries = Vec::with_capacity(counts.0 + counts.1);

    for (key, image) in &cache.image_cache {
        entries.push((
            *key,
            true,
            image.as_ref().map_or(0, |image| image.data.capacity()),
        ));
    }

    for (key, commands) in &cache.outline_command_cache {
        entries.push((
            *key,
            false,
            commands
                .as_ref()
                .map_or(0, |commands| mem::size_of_val(commands.as_ref())),
        ));
    }

    let mut bytes = entries
        .iter()
        .fold(0usize, |sum, entry| sum.saturating_add(entry.2));
    let mut count = entries.len();

    if count > MAX_GLYPH_CACHE_ENTRIES || bytes > MAX_GLYPH_CACHE_BYTES {
        entries.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.2));

        for (key, image, size) in entries {
            if count <= MAX_GLYPH_CACHE_ENTRIES * 3 / 4 && bytes <= MAX_GLYPH_CACHE_BYTES * 3 / 4 {
                break;
            }

            if image {
                cache.image_cache.remove(&key);
            } else {
                cache.outline_command_cache.remove(&key);
            }

            count -= 1;
            bytes = bytes.saturating_sub(size);
        }
        cache.image_cache.shrink_to_fit();
        cache.outline_command_cache.shrink_to_fit();
    }
    *checked = Some((cache.image_cache.len(), cache.outline_command_cache.len()));
}

#[cfg(test)]
mod cache_tests {
    use std::collections::BTreeSet;

    use cosmic_text::Weight;

    use crate::{style::TextStyle, text::rasterize_overlay_text};

    use super::*;

    fn key(glyph: u16) -> cosmic_text::CacheKey {
        cosmic_text::CacheKey::new(
            Default::default(),
            glyph,
            24.0,
            (0.0, 0.0),
            Weight::NORMAL,
            cosmic_text::CacheKeyFlags::empty(),
        )
        .0
    }

    #[test]
    fn retains_small_cache_but_releases_excess_entries() {
        let mut cache = SwashCache::new();
        cache.image_cache.insert(key(0), None);
        trim_glyph_cache(&mut cache, &mut None);
        assert_eq!(cache.image_cache.len(), 1);

        for glyph in 1..=MAX_GLYPH_CACHE_ENTRIES {
            cache.image_cache.insert(key(glyph as u16), None);
        }
        trim_glyph_cache(&mut cache, &mut None);
        assert_eq!(cache.image_cache.len(), MAX_GLYPH_CACHE_ENTRIES * 3 / 4);
        assert!(cache.outline_command_cache.is_empty());
    }

    #[test]
    fn releases_large_raster_even_with_few_entries() {
        let mut cache = SwashCache::new();
        cache.image_cache.insert(key(1), None);
        let mut image = cosmic_text::SwashImage::new();
        image.data.resize(MAX_GLYPH_CACHE_BYTES + 1, 0);
        cache.image_cache.insert(key(0), Some(image));
        trim_glyph_cache(&mut cache, &mut None);
        assert!(!cache.image_cache.contains_key(&key(0)));
        assert!(cache.image_cache.contains_key(&key(1)));
    }

    #[test]
    fn repeated_growth_retains_headroom_and_reuses_unchanged_cache() {
        let mut cache = SwashCache::new();
        let mut checked = None;

        for cycle in 0..20 {
            for glyph in 0..5000 {
                cache.image_cache.insert(key(glyph), None);
            }
            trim_glyph_cache(&mut cache, &mut checked);
            assert_eq!(
                cache.image_cache.len(),
                MAX_GLYPH_CACHE_ENTRIES * 3 / 4,
                "cycle {cycle}"
            );
            let keys: BTreeSet<_> = cache.image_cache.keys().copied().collect();
            trim_glyph_cache(&mut cache, &mut checked);
            assert_eq!(keys, cache.image_cache.keys().copied().collect());
        }
    }

    #[test]
    #[ignore = "manual render-time and cache-growth stress test"]
    fn changing_text_render_stress() {
        let started = std::time::Instant::now();
        let mut worst = std::time::Duration::ZERO;

        for index in 0..1000 {
            let config = TextStyle {
                font_size: (20 + index % 80) as f32,
                ..TextStyle::default()
            };
            let text = format!("Subtitle {index}: changing sizes — ÄÖÜ é 中文 العربية");
            let draw = std::time::Instant::now();
            rasterize_overlay_text(&config, &text, 1920, 1080).unwrap();
            worst = worst.max(draw.elapsed());
        }
        eprintln!(
            "1000 changing overlays: {:?}, slowest render: {:?}",
            started.elapsed(),
            worst
        );
    }
}
