use bytes::BytesMut;
use futures::{SinkExt, StreamExt};
use http::header::CONTENT_LENGTH;
use http::{header::HeaderValue, Method, Request, Response, StatusCode};
use serde_derive::{Deserialize, Serialize};
use std::{env, error::Error, fmt, io};
use tokio::{
    codec::{Decoder, Encoder, Framed},
    net::{TcpListener, TcpStream},
};

type Exception = Box<dyn Error + Sync + Send + 'static>;

#[tokio::main]
async fn main() -> Result<(), Exception> {
    // Parse the arguments, bind the TCP socket we'll be listening to, spin up
    // our worker threads, and start shipping sockets to those worker threads.
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());

    let mut listener = TcpListener::bind(&addr).await?;
    println!("Listening on: {}", addr);

    while let Ok((stream, remote_addr)) = listener.accept().await {
        tokio::spawn(async move {
            if let Err(e) = process(stream).await {
                println!(
                    "failed to process connection from {}; error = {}",
                    remote_addr, e
                );
            }
        });
    }

    Ok(())
}

async fn process(stream: TcpStream) -> Result<(), Exception> {
    let mut transport = Framed::new(stream, Http);

    while let Some(request) = transport.next().await {
        match request {
            Ok(request) => {
                let response = respond(request).await?;
                transport.send(response).await?;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

async fn respond(req: Request<String>) -> Result<Response<String>, Exception> {
    let mut response = Response::builder();

    let body = match (req.method(), req.uri().path()) {
        (&Method::POST, "/api") => {
            response.header("Content-Type", "application/json");

            let deserialize: Result<Vec<Command>, _> = serde_json::from_str(req.body());

            match deserialize {
                Ok(vec) => {
                    let cmds = Commands(vec);

                    cmds.execute().await?;

                    serde_json::to_string(&Message::ok())?
                }
                Err(e) => {
                    serde_json::to_string(&Message::err(format!("deserialize error: {}", e)))?
                }
            }
        }
        (&Method::GET, _) => {
            response.status(StatusCode::BAD_REQUEST);
            String::new()
        }
        _ => {
            response.status(StatusCode::NOT_FOUND);
            String::new()
        }
    };
    let response = response
        .body(body)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    Ok(response)
}

struct Http;

/// Implementation of encoding an HTTP response into a `BytesMut`, basically
/// just writing out an HTTP/1.1 response.
impl Encoder for Http {
    type Item = Response<String>;
    type Error = io::Error;

    fn encode(&mut self, item: Response<String>, dst: &mut BytesMut) -> io::Result<()> {
        use std::fmt::Write;

        write!(
            BytesWrite(dst),
            "\
             HTTP/1.1 {}\r\n\
             Server: Example\r\n\
             Content-Length: {}\r\n\
             Date: {}\r\n\
             ",
            item.status(),
            item.body().len(),
            date::now()
        )
        .unwrap();

        for (k, v) in item.headers() {
            dst.extend_from_slice(k.as_str().as_bytes());
            dst.extend_from_slice(b": ");
            dst.extend_from_slice(v.as_bytes());
            dst.extend_from_slice(b"\r\n");
        }

        dst.extend_from_slice(b"\r\n");
        dst.extend_from_slice(item.body().as_bytes());

        return Ok(());

        // Right now `write!` on `Vec<u8>` goes through io::Write and is not
        // super speedy, so inline a less-crufty implementation here which
        // doesn't go through io::Error.
        struct BytesWrite<'a>(&'a mut BytesMut);

        impl fmt::Write for BytesWrite<'_> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.0.extend_from_slice(s.as_bytes());
                Ok(())
            }

            fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
                fmt::write(self, args)
            }
        }
    }
}

/// Implementation of decoding an HTTP request from the bytes we've read so far.
/// This leverages the `httparse` crate to do the actual parsing and then we use
/// that information to construct an instance of a `http::Request` object,
/// trying to avoid allocations where possible.
impl Decoder for Http {
    type Item = Request<String>;
    type Error = Exception;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Request<String>>, Self::Error> {
        // TODO: we should grow this headers array if parsing fails and asks
        //       for more headers
        let mut headers = [None; 16];
        let (method, path, version, amt) = {
            let mut parsed_headers = [httparse::EMPTY_HEADER; 16];
            let mut r = httparse::Request::new(&mut parsed_headers);
            let status = r.parse(src).map_err(|e| {
                let msg = format!("failed to parse http request: {:?}", e);
                io::Error::new(io::ErrorKind::Other, msg)
            })?;

            let amt = match status {
                httparse::Status::Complete(amt) => amt,
                httparse::Status::Partial => return Ok(None),
            };

            let toslice = |a: &[u8]| {
                let start = a.as_ptr() as usize - src.as_ptr() as usize;
                assert!(start < src.len());
                (start, start + a.len())
            };

            for (i, header) in r.headers.iter().enumerate() {
                let k = toslice(header.name.as_bytes());
                let v = toslice(header.value);
                headers[i] = Some((k, v));
            }

            (
                toslice(r.method.unwrap().as_bytes()),
                toslice(r.path.unwrap().as_bytes()),
                r.version.unwrap(),
                amt,
            )
        };
        if version != 1 {
            return Err(io::Error::new(io::ErrorKind::Other, "only HTTP/1.1 accepted").into());
        }
        let data = src.split_to(amt).freeze();
        let mut ret = Request::builder();
        ret.method(&data[method.0..method.1]);
        ret.uri(data.slice(path.0, path.1));
        ret.version(http::Version::HTTP_11);
        for header in headers.iter() {
            let (k, v) = match *header {
                Some((ref k, ref v)) => (k, v),
                None => break,
            };
            let value = unsafe { HeaderValue::from_shared_unchecked(data.slice(v.0, v.1)) };
            ret.header(&data[k.0..k.1], value);
        }

        match ret.headers_ref() {
            Some(headers_ref) => match headers_ref.get(CONTENT_LENGTH) {
                Some(length) => {
                    let body_len: usize = length.to_str()?.parse()?;
                    let body = src.split_to(body_len).freeze();
                    Ok(Some(
                        ret.body(String::from_utf8(body.to_vec())?)
                            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?,
                    ))
                }
                None => Ok(Some(ret.body(String::new())?)),
            },
            None => Ok(Some(ret.body(String::new())?)),
        }
    }
}

mod date {
    use std::cell::RefCell;
    use std::fmt::{self, Write};
    use std::str;

    use time::{self, Duration};

    pub struct Now(());

    /// Returns a struct, which when formatted, renders an appropriate `Date`
    /// header value.
    pub fn now() -> Now {
        Now(())
    }

    // Gee Alex, doesn't this seem like premature optimization. Well you see
    // there Billy, you're absolutely correct! If your server is *bottlenecked*
    // on rendering the `Date` header, well then boy do I have news for you, you
    // don't need this optimization.
    //
    // In all seriousness, though, a simple "hello world" benchmark which just
    // sends back literally "hello world" with standard headers actually is
    // bottlenecked on rendering a date into a byte buffer. Since it was at the
    // top of a profile, and this was done for some competitive benchmarks, this
    // module was written.
    //
    // Just to be clear, though, I was not intending on doing this because it
    // really does seem kinda absurd, but it was done by someone else [1], so I
    // blame them!  :)
    //
    // [1]: https://github.com/rapidoid/rapidoid/blob/f1c55c0555007e986b5d069fe1086e6d09933f7b/rapidoid-commons/src/main/java/org/rapidoid/commons/Dates.java#L48-L66

    struct LastRenderedNow {
        bytes: [u8; 128],
        amt: usize,
        next_update: time::Timespec,
    }

    thread_local!(static LAST: RefCell<LastRenderedNow> = RefCell::new(LastRenderedNow {
        bytes: [0; 128],
        amt: 0,
        next_update: time::Timespec::new(0, 0),
    }));

    impl fmt::Display for Now {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            LAST.with(|cache| {
                let mut cache = cache.borrow_mut();
                let now = time::get_time();
                if now >= cache.next_update {
                    cache.update(now);
                }
                f.write_str(cache.buffer())
            })
        }
    }

    impl LastRenderedNow {
        fn buffer(&self) -> &str {
            str::from_utf8(&self.bytes[..self.amt]).unwrap()
        }

        fn update(&mut self, now: time::Timespec) {
            self.amt = 0;
            write!(LocalBuffer(self), "{}", time::at(now).rfc822()).unwrap();
            self.next_update = now + Duration::seconds(1);
            self.next_update.nsec = 0;
        }
    }

    struct LocalBuffer<'a>(&'a mut LastRenderedNow);

    impl fmt::Write for LocalBuffer<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let start = self.0.amt;
            let end = start + s.len();
            self.0.bytes[start..end].copy_from_slice(s.as_bytes());
            self.0.amt += s.len();
            Ok(())
        }
    }
}

type Percentage = u8;
type Millisecond = u64;

#[derive(Deserialize, Debug)]
#[serde(tag = "cmd")]
enum Command {
    #[serde(rename = "delay")]
    Delay {
        base: Millisecond,
        range: Option<Millisecond>,
        percentage: Option<Percentage>,
    },
    #[serde(rename = "loss")]
    Loss {
        base: Percentage,
        rate: Option<Percentage>,
    },
    #[serde(rename = "duplicate")]
    Duplicate { base: Percentage },
    #[serde(rename = "reorder")]
    Reorder {
        base: Percentage,
        related: Option<Percentage>,
    },
    #[serde(rename = "corrupt")]
    Corrupt { base: Percentage },
    #[serde(rename = "show")]
    Show,
}

#[derive(Serialize)]
struct Message {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl Message {
    fn ok() -> Self {
        Message {
            ok: true,
            message: None,
        }
    }

    fn err(message: String) -> Self {
        Message {
            ok: false,
            message: Some(message),
        }
    }
}

#[derive(Debug)]
struct Commands(Vec<Command>);

impl Commands {
    async fn execute(&self) -> Result<(), Exception> {
        println!("execute {:?}", self);
        Ok(())
    }
}
