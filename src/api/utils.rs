use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use simplelog::*;

use crate::api::{
    args_parse::Args,
    handles::{add_user, db_init},
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

        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password = args.password.unwrap();

        let password_hash = match argon2.hash_password(password.as_bytes(), &salt) {
            Ok(hash) => hash.to_string(),
            Err(e) => {
                error!("{e}");
                return Err(1);
            }
        };

        if let Err(e) = add_user(
            &args.email.unwrap(),
            &username,
            &password_hash.to_string(),
            &salt.to_string(),
            &1,
        )
        .await
        {
            error!("{e}");
            return Err(1);
        };

        info!("Create admin user \"{username}\" done...");

        return Err(0);
    }

    Ok(())
}
