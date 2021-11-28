use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context, Poll};

use actix_files::Files;
use actix_web::{App, HttpResponse, HttpServer, middleware, post, web};
use futures_core::Stream;
use path_absolutize::Absolutize;
use pin_project_lite::pin_project;
use tokio::process::Command;
use tokio_util::io::ReaderStream;

mod config;

pin_project! {
    #[derive(Debug)]
    struct OwnedResourceStream<T, R> {
        resource: T,
        #[pin]
        reader: R,
    }
}

impl<T, R> OwnedResourceStream<T, R> {
    pub fn new(resource: T, reader: R) -> Self {
        OwnedResourceStream {
            resource,
            reader,
        }
    }
}

impl<T, R> Stream for OwnedResourceStream<T, R>
    where R: Stream + Unpin
{
    type Item = R::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().reader.poll_next(cx)
    }
}

struct Config {
    scripts_path: PathBuf,
}

#[post("/{path:.*}")]
async fn scripts(path: web::Path<String>, config: web::Data<Config>, query: web::Query<Vec<(String, String)>>, form: web::Form<Vec<(String, String)>>) -> HttpResponse {
    let path = path.into_inner();
    let clean_path = match PathBuf::from(&path).absolutize_from(&config.scripts_path) {
        Ok(clean_path) => {
            match clean_path.is_file() {
                true => clean_path.into_owned(),
                false => {
                    println!("File not found: {}", clean_path.display());
                    return HttpResponse::NotFound()
                        .content_type(mime::TEXT_PLAIN_UTF_8)
                        .body("File not found");
                }
            }
        }
        Err(e) => {
            println!("Parse path failed: {}. Err: {:?}", path, e);
            return HttpResponse::NotFound()
                .content_type(mime::TEXT_PLAIN_UTF_8)
                .body("Parse path failed");
        }
    };

    let args: Vec<String> = query.into_inner().into_iter()
        .filter(|(k, _)| (k == "args[]" || k == "args"))
        .map(|(_, v)| v)
        .chain(
            form.into_inner().into_iter()
                .filter(|(k, _)| (k == "args[]" || k == "args"))
                .map(|(_, v)| v)
        ).collect();

    println!("Execute: {} args: {:?}", clean_path.display(), args);
    let mut cmd = Command::new(clean_path);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    cmd.kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            println!("Spawn cmd error: {:?}. Error: {:?}", cmd, e);
            return HttpResponse::InternalServerError()
                .content_type(mime::TEXT_PLAIN_UTF_8)
                .body("Spawn cmd error");
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            println!("Child did not have a handle to stdout: {:?}", cmd);
            return HttpResponse::InternalServerError()
                .content_type(mime::TEXT_PLAIN_UTF_8)
                .body("Child did not have a handle to stdout");
        }
    };
    let stream = ReaderStream::new(stdout);
    let owned_stream = OwnedResourceStream::new(child, stream);
    HttpResponse::Ok()
        .content_type(mime::TEXT_EVENT_STREAM)
        .streaming(owned_stream)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config_app = config::config();
    let matches = config_app.get_matches();
    let listen = matches.value_of("listen").expect("parse 'listen' error").to_owned();
    let public_path = matches.value_of("public").expect("parse 'public' error").to_owned();
    let public_index_filename = matches.value_of("public-index").expect("parse 'public-index' error").to_owned();
    let scripts_path = matches.value_of("scripts").expect("parse 'scripts' error").to_owned();

    let config = web::Data::new(Config {
        scripts_path: PathBuf::from(scripts_path).absolutize().unwrap().into_owned(),
    });

    println!("Server is listening on {}", listen);

    HttpServer::new(move || {
        let static_files = Files::new("/", public_path.clone())
            .index_file(public_index_filename.clone())
            .use_etag(true)
            .use_last_modified(true);

        App::new()
            .wrap(middleware::Logger::default())
            .app_data(config.clone())
            .service(scripts)
            .service(static_files)
    })
        .bind(listen)?
        .run()
        .await
}
