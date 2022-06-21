use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[clap(version,
    name = "ffpapi",
    version = "0.3.0",
    about = "ffplayout REST API",
    long_about = None)]
pub struct Args {
    #[clap(short, long, help = "Listen on IP:PORT, like: 127.0.0.1:8080")]
    pub listen: Option<String>,

    #[clap(short, long, help = "Initialize Database")]
    pub init: bool,

    #[clap(short, long, help = "Create admin user")]
    pub username: Option<String>,

    #[clap(short, long, help = "Admin email")]
    pub email: Option<String>,

    #[clap(short, long, help = "Admin password")]
    pub password: Option<String>,
}
