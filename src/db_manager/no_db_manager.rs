#![cfg(not(any(feature = "db-mongo", feature = "db-redis", feature = "db-sled")))]

use crate::config::DbConfig;
use crate::config::IndexConfig;
use crate::error::Error;
use crate::models::{Entry, Metadata, Query, Search, User};
use async_trait::async_trait;
use semver::Version;
use crate::models::Package;
use tokio::io::BufReader;
use crate::DbManager;
use tokio::io::AsyncBufReadExt;
use std::path::PathBuf;

#[derive(Debug)]
pub struct NoDbManager {
    path: PathBuf,
}

impl NoDbManager {
    pub fn new(config: &IndexConfig) -> Self {
        NoDbManager {
            path: config.local_path.clone(),
        }
    }
}

#[async_trait]
impl DbManager for NoDbManager {
    async fn new(_: &DbConfig) -> Result<Self, Error> {
        Ok(NoDbManager {
            path: PathBuf::from(""),
        })
    }

    async fn get_login_prefix(&self) -> Result<&str, Error> {
        Ok("no-db")
    }

    async fn can_edit_owners(&self, _: u32, _: &str) -> Result<bool, Error> {
        Ok(false)
    }

    async fn owners(&self, _: &str) -> Result<Vec<User>, Error> {
        Ok(vec![])
    }

    async fn add_owners(&self, _: &str, _: &[String]) -> Result<(), Error> {
        Ok(())
    }

    async fn remove_owners(&self, _: &str, _: &[String]) -> Result<(), Error> {
        Ok(())
    }

    async fn last_user_id(&self) -> Result<Option<u32>, Error> {
        Ok(None)
    }

    async fn user_id_for_token(&self, _: &str) -> Result<u32, Error> {
        Ok(0)
    }

    async fn token_by_login(&self, _: &str) -> Result<Option<String>, Error> {
        Ok(None)
    }

    async fn token_by_username(&self, _: &str) -> Result<Option<String>, Error> {
        Ok(None)
    }

    async fn set_token(&self, _: u32, _: &str) -> Result<(), Error> {
        Ok(())
    }

    async fn user_by_username(&self, _: &str) -> Result<User, Error> {
        Ok(User::default())
    }

    async fn user_by_login(&self, _: &str) -> Result<User, Error> {
        Ok(User::default())
    }

    async fn add_new_user(&self, _: User, _: &str) -> Result<(), Error> {
        Ok(())
    }

    async fn verify_password(&self, _: u32, _: &str) -> Result<bool, Error> {
        Ok(false)
    }

    async fn change_password(&self, _: u32, _: &str, _: &str) -> Result<(), Error> {
        Ok(())
    }

    async fn can_add_metadata(&self, _: u32, _: &str, _: Version) -> Result<bool, Error> {
        Ok(true)
    }

    async fn add_new_metadata(&self, _: u32, _: Metadata) -> Result<(), Error> {
        Ok(())
    }

    async fn can_edit_package(&self, _: u32, _: &str, _: Version) -> Result<bool,Error> {
        Ok(false)
    }

    async fn yank(&self, _: &str, _: Version) -> Result<(), Error> {
        Ok(())
    }

    async fn unyank(&self, _: &str, _: Version) -> Result<(), Error> {
        Ok(())
    }

    async fn search(&self, queried: &Query) -> Result<Search, Error> {
        Err(Error::CrateNotFoundInDb("".to_string()))
    }

    // Search for name and version at the given path
    async fn get_repo_url(&self, name: &str, version: Version) -> Result<Option<String>, Error> {
        // split name into parts, first part is first two letters, second part is next two letters
        let (na, rest) = name.clone().split_at(2);
        let (me, _) = rest.split_at(2);
        let path = format!("{}/{}/{}/{}", self.path.display(), na, me, name);

        println!("Path: {}", path);
        // Try open file at path
        let file = tokio::fs::File::open(path).await?;
        
        // Parse lines to Package and check for version
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            let package: Package = serde_json::from_str(&line.to_string()).map_err(|_| Error::ParsePackage)?;
            if package.vers == version {
                return Ok(Some(package.repository.unwrap().to_string()));
            }
        }

        Err(Error::VersionNotFoundInDb(version))
    }

    async fn insert_package(&self, _: &str, _: Entry) -> Result<(), Error> {
        Ok(())
    }
}