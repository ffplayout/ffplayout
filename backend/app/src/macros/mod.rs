#[macro_export]
macro_rules! vec_strings {
    ($($str:expr),* $(,)?) => {{
        vec![$($str.to_string()),*]
    }};
}

// #[cfg(tokio_unstable)]
// #[macro_export]
// macro_rules! named_spawn {
//     ($name:expr, $future:expr) => {
//         tokio::spawn(async {
//             tokio::task::Builder::new()
//                 .name($name)
//                 .spawn($future)
//                 .expect("failed to spawn task {}", $name)
//                 .await
//                 .expect(format!("task {} failed :(", $name))
//         });
//     };
// }

// #[cfg(tokio_unstable)]
// #[macro_export]
// macro_rules! named_spawn_blocking {
//     ($name:expr, $future:expr) => {
//         tokio::spawn(async {
//             tokio::task::Builder::new()
//                 .name($name)
//                 .spawn_blocking($future)
//                 .expect("failed to spawn task {}", $name)
//                 .await
//                 .expect(format!("task {} failed :(", $name))
//         });
//     };
// }
