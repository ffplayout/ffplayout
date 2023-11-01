use static_files::NpmBuild;

fn main() -> std::io::Result<()> {
    NpmBuild::new("../ffplayout-frontend")
        .install()?
        .run("generate")?
        .target("../ffplayout-frontend/.output/public")
        .change_detection()
        .to_resource_dir()
        .build()
}
