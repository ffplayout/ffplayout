use std::path::Path;

use regex::Regex;

pub fn validate_source(source: &str) -> bool
{
    let re = Regex::new(r"^https?://.*").unwrap();
    return Path::new(&source).is_file() || re.is_match(&source);
}