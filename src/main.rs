#[macro_use]
extern crate lazy_static;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use http::header::CONTENT_TYPE;
use http::{Method, Request, Response, StatusCode};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::fs::File;
use tokio::{
    codec::Framed,
    net::{TcpListener, TcpStream},
};

mod opt;
use opt::*;
mod error;
mod proto;
use error::*;
mod netem;
use netem::*;
mod utils;
use crate::proto::{Http, Resp};
use tokio::io::AsyncReadExt;
use utils::*;

#[tokio::main]
async fn main() -> WeoResult<()> {
    let opts: WeoOpts = WeoOpts::from_args();

    println!("{:?}", opts);

    let addr4 = SocketAddr::from(([0, 0, 0, 0], opts.port));

    let mut listener = TcpListener::bind(addr4).await?;
    println!("Listening on: {}", opts.port);

    let opts = Arc::new(opts);

    while let Ok((stream, remote_addr)) = listener.accept().await {
        let opts = opts.clone();
        tokio::spawn(async move {
            if let Err(e) = process(stream, opts.clone()).await {
                println!(
                    "failed to process connection from {}; error = {}",
                    remote_addr, e
                );
            }
        });
    }

    Ok(())
}

async fn process(stream: TcpStream, opts: Arc<WeoOpts>) -> WeoResult<()> {
    let mut transport = Framed::new(stream, proto::Http);

    while let Some(request) = transport.next().await {
        match request {
            Ok(request) => {
                let (resp, file) = respond(request, opts.clone()).await?;
                transport.send(resp).await?;

                if let Some(file) = file {
                    send_file(file, &mut transport).await?;
                }
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

async fn send_file(mut file: File, transport: &mut Framed<TcpStream, Http>) -> WeoResult<()> {
    let mut buff = [0u8; 1024];
    loop {
        match file.read(&mut buff).await {
            Ok(size) => {
                if size == 0 {
                    break;
                } else {
                    transport
                        .send(Resp::FileContent(Bytes::from(&buff[0..size])))
                        .await?;
                }
            }
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

async fn respond(req: Request<String>, opts: Arc<WeoOpts>) -> WeoResult<(Resp, Option<File>)> {
    let mut response = Response::builder();

    let result = match (req.method(), req.uri().path()) {
        (&Method::POST, "/api") => {
            response.header(CONTENT_TYPE, mime::APPLICATION_JSON.as_ref());

            let deserialize: Result<NetEm, _> = serde_json::from_str(req.body());

            let body = match deserialize {
                Ok(netem) => serde_json::to_string(&netem.execute().await)?.into(),
                Err(e) => {
                    serde_json::to_string(&Output::err_client(format!("deserialize error: {}", e)))?
                        .into()
                }
            };

            (Resp::Complete(response.body(body)?), None)
        }
        (&Method::GET, path) => {
            let path = if path.is_empty() || path == "/" {
                opts.root.join("index.html")
            } else {
                opts.root.join(&path[1..])
            };

            if let Some(mime) = get_mime(&path) {
                response.header(CONTENT_TYPE, mime);
            }

            let open = File::open(path).await;

            match open {
                Ok(file) => match file.metadata().await {
                    Ok(metadata) => (
                        Resp::FileHeader(response.body(())?, metadata.len()),
                        Some(file),
                    ),
                    Err(_) => {
                        response.status(StatusCode::NOT_FOUND);
                        (Resp::Complete(response.body(Bytes::new())?), None)
                    }
                },
                Err(_) => {
                    response.status(StatusCode::NOT_FOUND);
                    (Resp::Complete(response.body(Bytes::new())?), None)
                }
            }
        }
        _ => {
            response.status(StatusCode::NOT_FOUND);
            (Resp::Complete(response.body(Bytes::new())?), None)
        }
    };

    Ok(result)
}
