use static_files::NpmBuild;

fn main() -> std::io::Result<()> {
    if !cfg!(debug_assertions) {
        NpmBuild::new("../ffplayout-frontend")
            .install()?
            .run("generate")?
            .target("../ffplayout-frontend/.output/public")
            .change_detection()
            .to_resource_dir()
            .build()
    } else {
        Ok(())
    }
}
