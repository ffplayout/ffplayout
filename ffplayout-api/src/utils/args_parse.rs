use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[clap(version,
    about = "REST API for ffplayout",
    long_about = None)]
pub struct Args {
    #[clap(short, long, help = "ask for user credentials")]
    pub ask: bool,

    #[clap(long, help = "path to database file")]
    pub db: Option<String>,

    #[clap(short, long, help = "Listen on IP:PORT, like: 127.0.0.1:8787")]
    pub listen: Option<String>,

    #[clap(short, long, help = "Initialize Database")]
    pub init: bool,

    #[clap(short, long, help = "domain name for initialization")]
    pub domain: Option<String>,

    #[clap(short, long, help = "Create admin user")]
    pub username: Option<String>,

    #[clap(short, long, help = "Admin mail address")]
    pub mail: Option<String>,

    #[clap(short, long, help = "Admin password")]
    pub password: Option<String>,
}
