use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use http::header::CONTENT_TYPE;
use http::{Method, Request, Response, StatusCode};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
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
                let response = respond(request, opts.clone()).await?;
                transport.send(response).await?;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

async fn respond(req: Request<String>, opts: Arc<WeoOpts>) -> WeoResult<Response<Bytes>> {
    let mut response = Response::builder();

    let body = match (req.method(), req.uri().path()) {
        (&Method::POST, "/api") => {
            response.header(CONTENT_TYPE, mime::APPLICATION_JSON.as_ref());

            let deserialize: Result<NetEm, _> = serde_json::from_str(req.body());

            match deserialize {
                Ok(netem) => serde_json::to_string(&netem.execute().await)?.into(),
                Err(e) => serde_json::to_string(&Message::err_server(format!(
                    "deserialize error: {}",
                    e
                )))?
                .into(),
            }
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
                Ok(mut file) => {
                    let mut content = Vec::with_capacity(file.metadata().await?.len() as usize);
                    file.read_to_end(&mut content).await?;
                    Bytes::from(content)
                }
                Err(_) => {
                    response.status(StatusCode::NOT_FOUND);
                    Bytes::new()
                }
            }
        }
        _ => {
            response.status(StatusCode::NOT_FOUND);
            Bytes::new()
        }
    };
    let response = response
        .body(body)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    Ok(response)
}
