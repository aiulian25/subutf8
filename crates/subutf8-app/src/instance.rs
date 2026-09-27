use std::env;
use std::fs::{self, OpenOptions, Permissions};
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::constants::{
    API_PREFIX, DESKTOP_ADDRESS, HAND_OVER_TIMEOUT, INSTANCE_FILE_NAME, LOOPBACK_HOST, OPEN_ROUTE,
    PRIVATE_FILE_PERMISSIONS, PRIVATE_FOLDER_PERMISSIONS, RUNTIME_FOLDER_NAME,
    RUNTIME_FOLDER_VARIABLE, SUCCESS_STATUS_PREFIX, TOKEN_HEADER,
};

/// ACCESS-08: where the running app writes its address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InstanceRecord {
    port: u16,
    token: String,
}

#[derive(Serialize)]
struct OpenRequest<'paths> {
    paths: Vec<&'paths str>,
}

fn instance_file() -> Option<PathBuf> {
    let runtime = env::var_os(RUNTIME_FOLDER_VARIABLE)?;
    Some(
        PathBuf::from(runtime)
            .join(RUNTIME_FOLDER_NAME)
            .join(INSTANCE_FILE_NAME),
    )
}

/// ACCESS-08: hands the files to an app that is already running and asks it to show itself.
/// Returns false when no running app answers, so this launch starts its own.
pub async fn hand_over(files: &[PathBuf]) -> bool {
    let Some(record) = instance_file().and_then(|path| read_record(&path)) else {
        return false;
    };
    let paths = files.iter().filter_map(|path| path.to_str()).collect();
    let Ok(body) = serde_json::to_string(&OpenRequest { paths }) else {
        return false;
    };
    let handed_over = timeout(HAND_OVER_TIMEOUT, post_open(&record, &body)).await;
    handed_over.is_ok_and(|result| result.unwrap_or(false))
}

fn read_record(path: &Path) -> Option<InstanceRecord> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

async fn post_open(record: &InstanceRecord, body: &str) -> io::Result<bool> {
    let address = SocketAddr::from((Ipv4Addr::from(DESKTOP_ADDRESS), record.port));
    let mut stream = TcpStream::connect(address).await?;
    let request = format!(
        "POST {API_PREFIX}{OPEN_ROUTE} HTTP/1.1\r\nHost: {LOOPBACK_HOST}:{port}\r\n\
         {TOKEN_HEADER}: {token}\r\nContent-Type: application/json\r\n\
         Content-Length: {length}\r\nConnection: close\r\n\r\n{body}",
        port = record.port,
        token = record.token,
        length = body.len(),
    );
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    Ok(response.starts_with(SUCCESS_STATUS_PREFIX.as_bytes()))
}

/// ACCESS-08: the file is readable only by the user, in a folder only the user can enter.
pub fn record(port: u16, token: &str) -> io::Result<()> {
    let Some(path) = instance_file() else {
        return Ok(());
    };
    let folder = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(folder)?;
    fs::set_permissions(folder, Permissions::from_mode(PRIVATE_FOLDER_PERMISSIONS))?;
    let content = serde_json::to_string(&InstanceRecord {
        port,
        token: token.to_owned(),
    })
    .map_err(io::Error::other)?;
    let _ = fs::remove_file(&path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_PERMISSIONS)
        .open(&path)?;
    file.write_all(content.as_bytes())
}

/// Leaves the record alone when another copy of the app has written its own since.
pub fn remove(port: u16) {
    let Some(path) = instance_file() else {
        return;
    };
    if read_record(&path).is_some_and(|record| record.port == port) {
        let _ = fs::remove_file(path);
    }
}
