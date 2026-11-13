use std::{collections::HashMap, fs, path::Path};

/// Read declarations from our own master format. Missing or unfamiliar entries
/// remain unknown, so retained media never inherits an unverified declaration.
pub(crate) fn load_codecs(path: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let mut declarations = HashMap::new();
    let mut lines = text.lines();

    while let Some(line) = lines.next() {
        if !line.starts_with("#EXT-X-STREAM-INF:") {
            continue;
        }

        let Some(uri) = lines.next() else { break };
        let Some((_, value)) = line.split_once(",CODECS=\"") else {
            continue;
        };
        let Some((codecs, _)) = value.split_once('"') else {
            continue;
        };

        if !codecs.is_empty() {
            declarations.insert(uri.to_owned(), codecs.to_owned());
        }
    }

    declarations
}
