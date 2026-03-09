use std::env;

use static_files::NpmBuild;

fn main() -> std::io::Result<()> {
    if !cfg!(debug_assertions) && cfg!(feature = "embed_frontend") {
        let target_path = env::current_dir()?.join("../frontend/dist");

        NpmBuild::new("../")
            .install()?
            .run("build")?
            .target(target_path)
            .change_detection()
            .to_resource_dir()
            .build()
    } else {
        Ok(())
    }
}
