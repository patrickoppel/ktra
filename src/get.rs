use crate::db_manager::DbManager;
#[cfg(feature = "crates-io-mirroring")]
use crate::error::Error;
use crate::models::{Query, User};
use crate::utils::*;
use futures::TryFutureExt;
#[cfg(feature = "crates-io-mirroring")]
use reqwest::Client;
#[cfg(feature = "crates-io-mirroring")]
use semver::Version;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(feature = "crates-io-mirroring")]
use tokio::fs::OpenOptions;
#[cfg(feature = "crates-io-mirroring")]
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::{io::AsyncReadExt, sync::RwLock};
#[cfg(feature = "crates-io-mirroring")]
use url::Url;
#[cfg(feature = "crates-io-mirroring")]
use warp::http::Response;
#[cfg(feature = "crates-io-mirroring")]
use warp::hyper::body::Bytes;
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

#[cfg(not(feature = "crates-io-mirroring"))]
#[tracing::instrument(skip(db_manager, dl_dir_path, path))]
pub fn apis(
    db_manager: Arc<RwLock<impl DbManager>>,
    dl_dir_path: Arc<PathBuf>,
    path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let routes = download(dl_dir_path, path)
        .or(owners(db_manager.clone()))
        .or(search(db_manager));

    // With openid enabled, the `/me` route is handled in src/openid.rs
    #[cfg(not(feature = "openid"))]
    let routes = routes.or(me());

    routes
}

#[cfg(feature = "crates-io-mirroring")]
#[tracing::instrument(skip(db_manager, dl_dir_path, http_client, cache_dir_path, path))]
pub fn apis(
    db_manager: Arc<RwLock<impl DbManager>>,
    dl_dir_path: Arc<PathBuf>,
    http_client: Client,
    cache_dir_path: Arc<PathBuf>,
    path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let routes = download(dl_dir_path.clone(), path.clone())
        .or(download_github(db_manager.clone(), dl_dir_path))
        .or(download_crates_io(http_client, cache_dir_path))
        .or(owners(db_manager.clone()))
        .or(search(db_manager));
    // With openid enabled, the `/me` route is handled in src/openid.rs
    #[cfg(not(feature = "openid"))]
    let routes = routes.or(me());
    routes
}

#[tracing::instrument(skip(path))]
pub(crate) fn into_boxed_filters(path: Vec<String>) -> BoxedFilter<()> {
    let (h, t) = path.split_at(1);
    t.iter().fold(warp::path(h[0].clone()).boxed(), |accm, s| {
        accm.and(warp::path(s.clone())).boxed()
    })
}

#[tracing::instrument(skip(path, dl_dir_path))]
fn download(
    dl_dir_path: Arc<PathBuf>,
    path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    into_boxed_filters(path).and(warp::fs::dir(dl_dir_path.to_path_buf()))
}

// #[tracing::instrument(skip(path, db_manager, dl_dir_path))]
#[tracing::instrument(skip(db_manager, dl_dir_path))]
fn download_github(
    db_manager: Arc<RwLock<impl DbManager>>,
    dl_dir_path: Arc<PathBuf>
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone { 
    warp::get()
        .and(warp::path::tail())
        .and(with_db_manager(db_manager))
        .and(with_dl_dir_path(dl_dir_path))
        .and_then(handle_download_github)
}

// #[tracing::instrument(skip(db_manager, dl_dir_path, path))]
async fn handle_download_github(
    path: warp::path::Tail,
    db_manager: Arc<RwLock<impl DbManager>>,
    dl_dir_path: Arc<PathBuf>,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.read().await;
    
    let path_segments: Vec<&str> = path.as_str().split('/').collect();
    let crate_name = path_segments.get(1);
    let version = path_segments.get(2);

    tracing::debug!("crate_name: {:?}, version: {:?}", crate_name, version);

    if let (Some(crate_name), Some(version)) = (&crate_name, &version) {
        let crate_name = crate_name.to_string();
        let version = Version::parse(&version).map_err(|_| Error::SemVer)?;

        // Get the GitHub URL from the database
        let mut github_url = db_manager
            .get_repo_url(&crate_name, version.clone())
            .map_err(warp::reject::custom)
            .await?
            .ok_or_else(|| warp::reject::not_found())?;

        // Check if the URL ends in .git and if yes remove it
        if github_url.ends_with(".git") {
            let new_length = github_url.len() - 4;
            github_url.truncate(new_length);
        } 

        let base_url = "https://github.com/";
        let api_base_url = "https://api.github.com/repos/";

        if github_url.starts_with(base_url) {
            github_url = format!("{}{}", api_base_url, &github_url[base_url.len()..]);
        }
        github_url.push_str(&format!("/releases/tags/v{}",version));
        // tracing::debug!("github_url: {:?}", github_url);

        let contents = tokio::fs::read_to_string("ktra.toml").await.map_err(Error::Io)?;
        let config = toml::from_str::<crate::Config>(&contents).map_err(|_| warp::reject::custom(Error::Io(tokio::io::ErrorKind::Other.into())))?;
        let token_path = config.index_config.token_path.clone().ok_or_else(|| warp::reject::not_found())?;
        let mut token = tokio::fs::read_to_string(token_path).await.map_err(|_| warp::reject::custom(Error::Io(tokio::io::ErrorKind::Other.into())))?;

        token.pop(); // Remove the newline character

        // Get Asset URL from GitHub API (this is necessary for private repos, but works for public repos too)
        let client = reqwest::Client::builder()
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("token {}", token).parse().unwrap_or_else(|_| panic!("Failed to parse token")) // This unwrap should never fail,
                );
                headers.insert(
                    reqwest::header::USER_AGENT,
                    "rust web-api-client demo".parse().unwrap_or_else(|_| panic!("Failed to parse token")) // This unwrap should never fail,
                );
                headers
            })
            .build().map_err(|e| warp::reject::custom(Error::HttpRequest(e)))?;

        let response = client.get(&github_url).send().await.map_err(Error::HttpRequest)?;
        
        println!("Response: {:?}", response);
        
        let release = response
            .json::<serde_json::Value>()
            .await
            .map_err(Error::HttpRequest)?;
        let release_assets = release.get("assets").and_then(|a| a.as_array()).unwrap();

        let mut asset_url = "".to_string();
        for asset in release_assets {
            let name = asset.get("name").and_then(|n| n.as_str()).unwrap();
            if name.ends_with(".crate") {
                asset_url = asset.get("url").and_then(|u| u.as_str()).unwrap().to_string();
                break;
            }
        }

        if asset_url.is_empty() {
            return Err(warp::reject::not_found());
        }
        
        let client = reqwest::Client::builder()
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {}", token).parse().unwrap_or_else(|_| panic!("Failed to parse token")) // This unwrap should never fail,
                );
                headers.insert(
                    reqwest::header::ACCEPT,
                    "application/octet-stream".parse().unwrap_or_else(|_| panic!("Failed to parse token")) // This unwrap should never fail,
                );
                headers.insert(
                    reqwest::header::USER_AGENT,
                    "rust web-api-client demo".parse().unwrap_or_else(|_| panic!("Failed to parse token")) // This unwrap should never fail,
                );
                headers
            })
            .build().map_err(|e| warp::reject::custom(Error::HttpRequest(e)))?;
        let response = client.get(asset_url).send().await.map_err(Error::HttpRequest)?;

        // Get the bytes of the file
        let bytes = response.bytes().await.map_err(Error::HttpRequest)?;

        // Create the local file path
        let mut file_path = dl_dir_path.as_ref().to_path_buf();  
        file_path.push(format!("./{}/{}/{}-{}.crate", crate_name, version, crate_name, version));     

        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|_| Error::Io(tokio::io::ErrorKind::Other.into()))?;
        }
        // Write the file to disk
        tokio::fs::write(&file_path, &bytes).await.map_err(Error::Io)?;

        let response = Response::builder()
        .header("Content-Type", "application/x-tar")
        .body(bytes)
        .map_err(Error::HttpResponseBuilding)?;

        Ok(response)
    } else {
        Err(warp::reject::not_found())
    }
}

#[cfg(feature = "crates-io-mirroring")]
#[tracing::instrument(skip(http_client, cache_dir_path, crate_name, version))]
async fn cache_crate_file(
    http_client: Client,
    cache_dir_path: Arc<PathBuf>,
    crate_name: impl AsRef<str>,
    version: Version,
) -> Result<Bytes, Rejection> {
    let computation = async move {
        let mut cache_dir_path = cache_dir_path.as_ref().to_path_buf();
        let crate_components = format!("{}/{}/download", crate_name.as_ref(), version);
        cache_dir_path.push(&crate_components);
        let cache_file_path = cache_dir_path;

        if file_exists_and_not_empty(&cache_file_path).await {
            OpenOptions::new()
                .write(false)
                .create(false)
                .read(true)
                .open(cache_file_path)
                .and_then(|mut file| async move {
                    let mut buffer = Vec::new();
                    file.read_to_end(&mut buffer).await?;
                    Ok(Bytes::from(buffer))
                })
                .map_err(Error::Io)
                .await
        } else {
            let mut crate_dir_path = cache_file_path.clone();
            crate_dir_path.pop();
            let crate_dir_path = crate_dir_path;

            tokio::fs::create_dir_all(crate_dir_path)
                .map_err(Error::Io)
                .await?;

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .read(true)
                .open(&cache_file_path)
                .map_err(Error::Io)
                .await?;
            let mut file = BufWriter::with_capacity(128 * 1024, file);

            let crates_io_base_url =
                Url::parse("https://crates.io/api/v1/crates/").map_err(Error::UrlParsing)?;
            let crate_file_url = crates_io_base_url
                .join(&crate_components)
                .map_err(Error::UrlParsing)?;
            let body = http_client
                .get(crate_file_url)
                .send()
                .and_then(|res| async move { res.error_for_status() })
                .and_then(|res| res.bytes())
                .map_err(Error::HttpRequest)
                .await?;

            if body.is_empty() {
                return Err(Error::InvalidHttpResponseLength);
            }

            file.write_all(&body).map_err(Error::Io).await?;
            file.flush().map_err(Error::Io).await?;

            Ok(body)
        }
    };

    computation.map_err(warp::reject::custom).await
}

#[cfg(feature = "crates-io-mirroring")]
#[tracing::instrument(skip(cache_dir_path))]
fn download_crates_io(
    http_client: Client,
    cache_dir_path: Arc<PathBuf>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_http_client(http_client))
        .and(with_cache_dir_path(cache_dir_path))
        .and(warp::path!(
            "ktra" / "api" / "v1" / "mirror" / String / Version / "download"
        ))
        .and_then(cache_crate_file)
        .and_then(handle_download_crates_io)
}

#[cfg(feature = "crates-io-mirroring")]
#[tracing::instrument(skip(crate_file_data))]
async fn handle_download_crates_io(crate_file_data: Bytes) -> Result<impl Reply, Rejection> {
    let response = Response::builder()
        .header("Content-Type", "application/x-tar")
        .body(crate_file_data)
        .map_err(Error::HttpResponseBuilding)?;

    Ok(response)
}

#[tracing::instrument(skip(db_manager))]
fn owners(
    db_manager: Arc<RwLock<impl DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(authorization_header())
        .and(warp::path!("api" / "v1" / "crates" / String / "owners"))
        .and_then(handle_owners)
}

#[tracing::instrument(skip(db_manager, _token, name))]
async fn handle_owners(
    db_manager: Arc<RwLock<impl DbManager>>,
    // `token` is not a used argument.
    // the specification demands that the authorization is required but listing owners api does not update the database.
    _token: String,
    name: String,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.read().await;
    let owners = db_manager
        .owners(&name)
        .map_err(warp::reject::custom)
        .await?;
    Ok(owners_json(owners))
}

#[tracing::instrument(skip(db_manager))]
fn search(
    db_manager: Arc<RwLock<impl DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(warp::path!("api" / "v1" / "crates"))
        .and(warp::query::<Query>())
        .and_then(handle_search)
}

#[tracing::instrument(skip(db_manager, query))]
async fn handle_search(
    db_manager: Arc<RwLock<impl DbManager>>,
    query: Query,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.read().await;
    db_manager
        .search(&query)
        .map_ok(|s| warp::reply::json(&s))
        .map_err(warp::reject::custom)
        .await
}

#[tracing::instrument]
fn me() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(warp::path!("me"))
        .map(|| "$ curl -X POST -H 'Content-Type: application/json' -d '{\"password\":\"YOUR PASSWORD\"}' https://<YOURDOMAIN>/ktra/api/v1/login/<YOUR USERNAME>")
}

#[tracing::instrument(skip(owners))]
fn owners_json(owners: Vec<User>) -> impl Reply {
    warp::reply::json(&serde_json::json!({ "users": owners }))
}
