use std::{env, path::Path};

use static_files::NpmBuild;

fn main() -> std::io::Result<()> {
    let gen_path = Path::new(&env::var("OUT_DIR").unwrap()).join("generated.rs");

    if Ok("release".to_owned()) == env::var("PROFILE") || !gen_path.is_file() {
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
