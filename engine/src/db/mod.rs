use std::io::{stdin, stdout, Write};

use sqlx::{migrate::MigrateDatabase, Pool, Sqlite, SqlitePool};

pub mod handles;
pub mod models;

use crate::utils::db_path;

pub async fn db_pool() -> Result<Pool<Sqlite>, sqlx::Error> {
    let db_path = db_path().unwrap();

    if !Sqlite::database_exists(db_path).await.unwrap_or(false) {
        Sqlite::create_database(db_path).await.unwrap();
    }

    let conn = SqlitePool::connect(db_path).await?;

    Ok(conn)
}

pub async fn db_drop() {
    let mut drop_answer = String::new();

    print!("Drop Database [Y/n]: ");
    stdout().flush().unwrap();

    stdin()
        .read_line(&mut drop_answer)
        .expect("Did not enter a yes or no?");

    let drop = drop_answer.trim().to_lowercase().starts_with('y');

    if drop {
        match Sqlite::drop_database(db_path().unwrap()).await {
            Ok(_) => println!("Successfully dropped DB"),
            Err(e) => eprintln!("{e}"),
        };
    };
}
