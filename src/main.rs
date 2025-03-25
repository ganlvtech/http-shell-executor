use axum::extract::{OriginalUri, Query};
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Extension, Form, Router};
use bytes::Bytes;
use clap::Parser;
use http_body::Frame;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};
use tokio::process::Command;
use tower_http::services::{ServeDir, ServeFile};

pub fn path_clean(path: impl AsRef<str>) -> String {
    let mut out = Vec::<&str>::new();
    for comp in path.as_ref().split("/") {
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
    _resource: T,
    reader: R,
}

impl<T, R> OwnedResourceStream<T, R> {
    pub fn new(resource: T, reader: R) -> Self {
        OwnedResourceStream {
            _resource: resource,
            reader,
        }
    }
}

// impl<T, R: AsyncRead> Stream for OwnedResourceStream<T, R> {
//     type Item = Result<Bytes, std::io::Error>;
//
//     fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         let mut buf = BytesMut::with_capacity(4096);
//         let reader = self.project().reader;
//         let mut reader = Pin::new(reader);
//         match reader.poll_read(cx, &mut buf) {
//             Poll::Ready(Ok(())) => {
//                 if buf.is_empty() {
//                     Poll::Ready(None)
//                 } else {
//                     Poll::Ready(Some(Ok(buf.freeze())))
//                 }
//             }
//             Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
//             Poll::Pending => Poll::Pending,
//         }
//     }
// }

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

struct AppConfig {
    scripts_path: PathBuf,
}

async fn scripts(Extension(config): Extension<Arc<AppConfig>>, OriginalUri(original_uri): OriginalUri, Query(query_params): Query<Vec<(String, String)>>, Form(form_params): Form<Vec<(String, String)>>) -> impl IntoResponse {
    let args: Vec<String> = query_params.iter().chain(form_params.iter())
        .filter(|(k, _)| k == "args[]" || k == "args")
        .map(|(_, v)| v.to_owned())
        .collect();
    let clean_path = path_clean(original_uri.path());
    let script_path = config.scripts_path.join(&clean_path);
    println!("Clean Path: {} Script Path: {} args: {:?}", &clean_path, script_path.to_string_lossy(), args);

    let mut cmd = Command::new(script_path);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    cmd.kill_on_drop(true);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            println!("Spawn cmd error: {:?}. Error: {:?}", cmd, e);
            return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "text/plain")
                .body("Spawn cmd error".to_string())
                .unwrap().into_response();
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            println!("Child did not have a handle to stdout: {:?}", cmd);
            return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "text/plain")
                .body("Child did not have a handle to stdout".to_string())
                .unwrap().into_response();
        }
    };
    let owned_stream = OwnedResourceStream::new(child, stdout);
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain")
        .header("X-Content-Type-Options", "nosniff")
        .body(owned_stream)
        .unwrap().into_response()
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, value_name = "IP:PORT", help = "Listening IP and port", default_value = "127.0.0.1:80")]
    listen: String,
    #[arg(long, value_name = "DIR", help = "Static files directory path", default_value = "./public/")]
    public: String,
    #[arg(long, value_name = "FILE", help = "Index filename", default_value = "./public/index.html")]
    public_index: String,
    #[arg(long, value_name = "DIR", help = "Shell scripts directory path", default_value = "./scripts/")]
    scripts: String,
}

#[tokio::main]
async fn main() {
    let config = Args::parse();
    let app_config = Arc::new(AppConfig {
        scripts_path: PathBuf::from(config.scripts),
    });
    let app = Router::new()
        .route("/{*path}", post(scripts))
        .layer(Extension(app_config))
        .fallback_service(ServeDir::new(&config.public).fallback(ServeFile::new(&config.public_index)));

    let listener = tokio::net::TcpListener::bind(&config.listen).await.unwrap();
    println!("Server is listening on {}", &config.listen);
    axum::serve(listener, app).await.unwrap();
}
