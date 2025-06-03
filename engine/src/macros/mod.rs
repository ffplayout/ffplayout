#[macro_export]
macro_rules! vec_strings {
    ($($str:expr),* $(,)?) => {{
        vec![$($str.to_string()),*]
    }};
}
