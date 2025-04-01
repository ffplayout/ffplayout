use static_files::NpmBuild;

fn main() -> std::io::Result<()> {
    if !cfg!(debug_assertions) && cfg!(feature = "embed_frontend") {
        NpmBuild::new("../")
            .install()?
            .run("build")?
            .target("../frontend/dist")
            .change_detection()
            .to_resource_dir()
            .build()
    } else {
        Ok(())
    }
}
