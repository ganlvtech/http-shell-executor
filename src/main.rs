use axum::extract::{DefaultBodyLimit, OriginalUri, Query};
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::{Extension, Form};
use bytes::{Bytes};
use clap::Parser;
use http_body::Frame;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::task::{Context, Poll};
use axum::routing::{post, put};
use tokio::fs::{create_dir_all, File};
use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};
use tokio::process::Command;
use tower_http::services::{ServeDir, ServeFile};

pub fn path_clean(path: impl AsRef<str>) -> String {
    let mut out = Vec::<&str>::new();
    let path = path.as_ref().replace("\\", "/");
    for comp in path.split("/") {
        match comp {
            "" => (),
            "." => (),
            ".." => {
                out.pop();
            }
            _ => out.push(comp),
        }
    }
    out.join("/")
}

#[derive(Debug)]
struct OwnedResourceStream<T, R> {
    name: String,
    _resource: T,
    reader: R,
}

impl<T, R> OwnedResourceStream<T, R> {
    pub fn new(name: String, resource: T, reader: R) -> Self {
        OwnedResourceStream {
            name,
            _resource: resource,
            reader,
        }
    }
}

impl<T, R> Drop for OwnedResourceStream<T, R> {
    fn drop(&mut self) {
        eprintln!("OwnedResourceStream {} dropped", self.name);
    }
}

impl<T: Unpin, R: AsyncRead + Unpin> http_body::Body for OwnedResourceStream<T, R> {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let mut buf = [0u8; 1024];
        let mut read_buf = ReadBuf::new(&mut buf);
        let reader = &mut this.reader;
        let reader = Pin::new(reader);
        match reader.poll_read(cx, &mut read_buf) {
            Poll::Ready(Ok(())) => {
                if read_buf.filled().is_empty() {
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(Ok(Frame::data(Bytes::from(read_buf.filled().to_vec())))))
                }
            }
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

async fn handle_scripts(Extension(config): Extension<Arc<Args>>, OriginalUri(original_uri): OriginalUri, Query(query_params): Query<Vec<(String, String)>>, Form(form_params): Form<Vec<(String, String)>>) -> impl IntoResponse {
    let path = match urlencoding::decode(original_uri.path()) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Invalid path: {}", e);
            return (StatusCode::BAD_REQUEST, format!("Invalid path: {}", e)).into_response();
        }
    };
    #[allow(unused_mut)]
    let mut args: Vec<String> = query_params.iter().chain(form_params.iter())
        .filter(|(k, _)| k == "args[]" || k == "args")
        .map(|(_, v)| v.to_owned())
        .collect();
    let clean_path = path_clean(path);
    #[allow(unused_mut)]
    let mut script_path = PathBuf::from(&config.scripts_dir).join(&clean_path);
    eprintln!("Clean Path: {} Script Path: {} args: {:?}", &clean_path, script_path.to_string_lossy(), args);
    if !script_path.exists() {
        return Response::builder().status(StatusCode::NOT_FOUND)
            .header("Content-Type", "text/plain")
            .body("Not Found".to_string())
            .unwrap().into_response();
    }
    #[cfg(windows)]
    if script_path.to_string_lossy().ends_with(".sh") {
        let bash_path = PathBuf::from("C:\\Program Files\\Git\\bin\\bash.exe");
        if bash_path.exists() {
            args.insert(0, script_path.to_string_lossy().into_owned());
            script_path = bash_path;
        }
    }

    let name = format!("{} {:?}", script_path.to_string_lossy(), args);
    let mut cmd = Command::new(script_path);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    cmd.kill_on_drop(true);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Spawn cmd error: {:?}. Error: {:?}", cmd, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Spawn cmd error").into_response();
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            eprintln!("Child did not have a handle to stdout: {:?}", cmd);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Child did not have a handle to stdout").into_response();
        }
    };
    let owned_stream = OwnedResourceStream::new(name, child, stdout);
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("X-Content-Type-Options", "nosniff")
        .body(owned_stream)
        .unwrap().into_response()
}

async fn handle_upload(Extension(config): Extension<Arc<Args>>, OriginalUri(original_uri): OriginalUri, body: Bytes) -> Result<(), (StatusCode, String)> {
    if !config.upload {
        return Err((StatusCode::FORBIDDEN, "Upload is disabled".to_string()));
    }
    let path = urlencoding::decode(original_uri.path()).map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid path: {}", e)))?;
    let path = match path.strip_prefix(&config.upload_path_prefix) {
        Some(path) => path,
        None => return Err((StatusCode::FORBIDDEN, format!("Invalid path: {}", path))),
    };
    let full_path = PathBuf::from(&config.upload_dir).join(path_clean(path));
    eprintln!("Upload {} bytes to: {:?}", body.len(), full_path);
    if let Some(parent) = full_path.parent() {
        create_dir_all(parent).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create directories: {}", e)))?;
    }
    File::create(&full_path).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create file: {}", e)))?
        .write_all(&body).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write file: {}", e)))?;
    Ok(())
}

#[derive(Parser, Clone, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, value_name = "IP:PORT", help = "Listening IP and port", default_value = "127.0.0.1:80")]
    listen: String,
    #[arg(long, value_name = "DIR", help = "Static files directory path", default_value = "./public/")]
    public_dir: String,
    #[arg(long, value_name = "FILE", help = "Index filename", default_value = "./public/index.html")]
    public_index: String,
    #[arg(long, value_name = "DIR", help = "Shell scripts directory path", default_value = "./scripts/")]
    scripts_dir: String,
    #[arg(long, help = "Enable file upload", default_value = "false")]
    upload: bool,
    #[arg(long, value_name = "DIR", help = "Upload directory path", default_value = "./public/uploads")]
    upload_dir: String,
    #[arg(long, value_name = "PREFIX", help = "Upload URL path prefix", default_value = "/uploads")]
    upload_path_prefix: String,
}

#[tokio::main]
async fn main() {
    let config = Args::parse();
    let app = post(handle_scripts)
        .merge(put(handle_upload).layer(DefaultBodyLimit::max(200 * 1024 * 1024)))
        .fallback_service(ServeDir::new(&config.public_dir).fallback(ServeFile::new(&config.public_index)))
        .layer(Extension(Arc::new(config.clone())));

    let listener = tokio::net::TcpListener::bind(&config.listen).await.unwrap();
    println!("Server is listening on {}", &config.listen);
    axum::serve(listener, app).await.unwrap();
}
