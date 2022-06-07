use sha_crypt::{sha512_simple, Sha512Params};
use simplelog::*;

use crate::api::{
    args_parse::Args,
    handles::{add_user, db_connection, db_init},
};

pub async fn run_args(args: Args) -> Result<(), i32> {
    if !args.init && args.listen.is_none() && args.username.is_none() {
        error!("Wrong number of arguments! Run ffpapi --help for more information.");

        return Err(0);
    }

    if args.init {
        if let Err(e) = db_init().await {
            panic!("{e}");
        };

        return Err(0);
    }

    if let Some(username) = args.username {
        if args.email.is_none() || args.password.is_none() {
            error!("Email/password missing!");
            return Err(1);
        }

        let params = Sha512Params::new(10_000).expect("RandomError!");

        let hashed_password =
            sha512_simple(&args.password.unwrap(), &params).expect("Should not fail");

        match db_connection().await {
            Ok(pool) => {
                if let Err(e) =
                    add_user(&pool, &args.email.unwrap(), &username, &hashed_password, &1).await
                {
                    pool.close().await;
                    error!("{e}");
                    return Err(1);
                };

                pool.close().await;
                info!("Create admin user \"{username}\" done...");

                return Err(0);
            }
            Err(e) => {
                error!("Add admin user failed! Did you init the database?");
                panic!("{e}")
            }
        }
    }

    Ok(())
}
