use base64::{engine::general_purpose::STANDARD, Engine};
use http_body_util::BodyExt;
use hudsucker::{
    hyper::body::{Body as HttpBody, Bytes, Frame, SizeHint},
    Body, Error,
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tracing::trace;

struct TraceBody {
    inner: Body,
    request_id: u64,
    direction: &'static str,
    offset: u64,
}

pub fn wrap(body: Body, request_id: u64, direction: &'static str) -> Body {
    if !tracing::enabled!(target: "shanten_backend::http", tracing::Level::TRACE) {
        return body;
    }
    Body::from(
        TraceBody {
            inner: body,
            request_id,
            direction,
            offset: 0,
        }
        .boxed(),
    )
}

impl HttpBody for TraceBody {
    type Data = Bytes;
    type Error = Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Error>>> {
        let result = Pin::new(&mut self.inner).poll_frame(cx);
        let request_id = self.request_id;
        let direction = self.direction;
        match &result {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    // Bound each log entry while retaining the entire streamed body.
                    for chunk in data.chunks(8 * 1024) {
                        let (encoding, content) = match std::str::from_utf8(chunk) {
                            Ok(text) => ("utf-8", text.to_owned()),
                            Err(_) => ("base64", STANDARD.encode(chunk)),
                        };
                        trace!(target: "shanten_backend::http", request_id, direction,
                            offset = self.offset, bytes = chunk.len(), encoding, content,
                            "HTTP body chunk");
                        self.offset += chunk.len() as u64;
                    }
                }
                if let Some(trailers) = frame.trailers_ref() {
                    trace!(target: "shanten_backend::http", request_id, direction, ?trailers, "HTTP trailers");
                }
            }
            Poll::Ready(Some(Err(error))) => {
                trace!(target: "shanten_backend::http", request_id, direction, ?error, "HTTP body read failed");
            }
            _ => {}
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hudsucker::{futures::stream, hyper::HeaderMap};
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct Output(Arc<Mutex<Vec<u8>>>);
    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn trace_preserves_stream_frames_errors_and_size() {
        let output = Output::default();
        let writer = output.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let mut full = wrap(Body::from("hello"), 41, "outbound");
        assert_eq!(full.size_hint().exact(), Some(5));
        assert_eq!(
            full.frame().await.unwrap().unwrap().into_data().unwrap(),
            "hello"
        );
        assert!(full.is_end_stream());

        let data = Bytes::from(vec![0xff; 8193]);
        let mut trailers = HeaderMap::new();
        trailers.insert("x-checksum", "test".parse().unwrap());
        let body = Body::from(http_body_util::StreamBody::new(stream::iter(vec![
            Ok(Frame::data(data.clone())),
            Ok(Frame::trailers(trailers.clone())),
            Err(Error::Io(std::io::Error::other("stream failed"))),
        ])));
        let mut body = wrap(body, 42, "inbound");
        assert_eq!(
            body.frame().await.unwrap().unwrap().into_data().unwrap(),
            data
        );
        assert_eq!(
            body.frame()
                .await
                .unwrap()
                .unwrap()
                .into_trailers()
                .unwrap(),
            trailers
        );
        assert!(body.frame().await.unwrap().is_err());
        assert!(body.frame().await.is_none());
        let logs = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("TRACE"));
        assert!(logs.contains("hello"));
        assert!(logs.contains("request_id=42"));
        assert!(logs.contains("base64"));
        assert_eq!(logs.matches("HTTP body chunk").count(), 3);
        assert!(logs.contains("offset=8192"));
        assert!(logs.contains("x-checksum"));
        assert!(logs.contains("stream failed"));

        let quiet = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .finish();
        let _quiet_guard = tracing::subscriber::set_default(quiet);
        let untouched = wrap(Body::from("unlogged"), 43, "outbound");
        assert_eq!(untouched.size_hint().exact(), Some(8));
        assert_eq!(untouched.collect().await.unwrap().to_bytes(), "unlogged");
        assert_eq!(
            String::from_utf8(output.0.lock().unwrap().clone()).unwrap(),
            logs
        );
    }
}
