use crate::Exception;
use bytes::{Bytes, BytesMut};
use http::header::CONTENT_LENGTH;
use http::{HeaderMap, HeaderValue, Request, Response};
use std::{fmt, io};
use tokio_util::codec::{Decoder, Encoder};

pub struct Http;
pub enum Resp {
    Complete(Response<Bytes>),
    FileHeader(Response<()>, u64),
    FileContent(Bytes),
}

/// Implementation of encoding an HTTP response into a `BytesMut`, basically
/// just writing out an HTTP/1.1 response.
impl Encoder<Resp> for Http {
    type Error = Exception;

    fn encode(&mut self, item: Resp, dst: &mut BytesMut) -> Result<(), Exception> {
        use std::fmt::Write;

        match item {
            Resp::Complete(response) => {
                write!(
                    BytesWrite(dst),
                    "\
                     HTTP/1.1 {}\r\n\
                     Server: weo\r\n\
                     Content-Length: {}\r\n\
                     Accept-Ranges: bytes\r\n\
                     Access-Control-Allow-Origin: *\r\n\
                     Access-Control-Allow-Headers: *\r\n\
                     Access-Control-Allow-Methods: *\r\n\
                     Connection: keep-alive\r\n\
                     Date: {}\r\n\
                     ",
                    response.status(),
                    response.body().len(),
                    date::now()
                )?;

                extend_dst(dst, response.headers());

                dst.extend_from_slice(response.body().as_ref());
            }
            Resp::FileHeader(response, file_size) => {
                write!(
                    BytesWrite(dst),
                    "\
                     HTTP/1.1 {}\r\n\
                     Server: weo\r\n\
                     Content-Length: {}\r\n\
                     Accept-Ranges: bytes\r\n\
                     Access-Control-Allow-Origin: *\r\n\
                     Access-Control-Allow-Headers: *\r\n\
                     Access-Control-Allow-Methods: *\r\n\
                     Connection: keep-alive\r\n\
                     Date: {}\r\n\
                     ",
                    response.status(),
                    file_size,
                    date::now()
                )?;

                extend_dst(dst, response.headers());
            }
            Resp::FileContent(bytes) => {
                dst.extend_from_slice(bytes.as_ref());
            }
        }

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

        fn extend_dst(dst: &mut BytesMut, headers: &HeaderMap<HeaderValue>) {
            for (k, v) in headers {
                dst.extend_from_slice(k.as_str().as_bytes());
                dst.extend_from_slice(b": ");
                dst.extend_from_slice(v.as_bytes());
                dst.extend_from_slice(b"\r\n");
            }

            dst.extend_from_slice(b"\r\n");
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

            let to_slice = |a: &[u8]| {
                let start = a.as_ptr() as usize - src.as_ptr() as usize;
                assert!(start < src.len());
                (start, start + a.len())
            };

            for (i, header) in r.headers.iter().enumerate() {
                let k = to_slice(header.name.as_bytes());
                let v = to_slice(header.value);
                headers[i] = Some((k, v));
            }

            (
                to_slice(r.method.unwrap().as_bytes()),
                to_slice(r.path.unwrap().as_bytes()),
                r.version.unwrap(),
                amt,
            )
        };
        if version != 1 {
            return Err(io::Error::new(io::ErrorKind::Other, "only HTTP/1.1 accepted").into());
        }

        let data = src.split_to(amt).freeze();
        let mut builder = Request::builder()
            .method(&data[method.0..method.1])
            .uri(String::from_utf8(data.slice(path.0..path.1).to_vec())?)
            .version(http::Version::HTTP_11);
        for header in headers.iter() {
            let (k, v) = match *header {
                Some((ref k, ref v)) => (k, v),
                None => break,
            };
            let value = HeaderValue::from_bytes(&data.slice(v.0..v.1))?;
            builder = builder.header(&data[k.0..k.1], value);
        }

        match builder.headers_ref() {
            Some(headers_ref) => match headers_ref.get(CONTENT_LENGTH) {
                Some(length) => {
                    let body_len: usize = length.to_str()?.parse()?;

                    if body_len > src.len() {
                        return Ok(None);
                    }

                    let body = src.split_to(body_len).freeze();
                    Ok(Some(
                        builder
                            .body(String::from_utf8(body.to_vec())?)
                            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?,
                    ))
                }
                None => Ok(Some(builder.body(String::new())?)),
            },
            None => Ok(Some(builder.body(String::new())?)),
        }
    }
}

mod date {
    use std::cell::RefCell;
    use std::fmt::{self, Write};
    use std::str;

    use chrono::{DateTime, Duration, Local, TimeZone};

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
        next_update: DateTime<Local>,
    }

    thread_local!(static LAST: RefCell<LastRenderedNow> = RefCell::new(LastRenderedNow {
        bytes: [0; 128],
        amt: 0,
        next_update: Local.timestamp(0 ,0),
    }));

    impl fmt::Display for Now {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            LAST.with(|cache| {
                let mut cache = cache.borrow_mut();
                let now = Local::now();
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

        fn update(&mut self, now: DateTime<Local>) {
            self.amt = 0;
            write!(LocalBuffer(self), "{}", now.to_rfc2822()).unwrap();
            self.next_update = now + Duration::seconds(1);
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
