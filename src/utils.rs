use std::path::Path;

pub fn get_mime<P: AsRef<Path>>(path: P) -> Option<&'static str> {
    if let Some(ext) = path.as_ref().extension() {
        match ext.to_str() {
            Some("js") => Some(mime::APPLICATION_JAVASCRIPT.as_ref()),
            Some("css") => Some(mime::TEXT_CSS.as_ref()),
            Some("html") | Some("htm") => Some(mime::TEXT_HTML_UTF_8.as_ref()),
            Some("ico") => Some("image/vnd.microsoft.icon"),
            _ => None,
        }
    } else {
        None
    }
}
