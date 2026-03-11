use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;

/// Minimal HTTP POST using raw TcpStream + native-tls (or plain TCP).
///
/// `extra_headers` — fully formed header lines (e.g. `"Authorization: Bearer xxx"`).
pub fn http_post(
    base_url: &str,
    path: &str,
    extra_headers: &[String],
    body: &str,
) -> Result<String, String> {
    let url = format!("{base_url}{path}");

    let is_https = url.starts_with("https://");
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("invalid URL scheme")?;

    let (host_port, request_path) = without_scheme
        .split_once('/')
        .map(|(h, p)| (h, format!("/{p}")))
        .unwrap_or((without_scheme, path.to_string()));

    let (host, port) = if host_port.contains(':') {
        let (h, p) = host_port.rsplit_once(':').unwrap();
        (h, p.parse::<u16>().map_err(|e| e.to_string())?)
    } else if is_https {
        (host_port, 443)
    } else {
        (host_port, 80)
    };

    let mut header_block = format!(
        "POST {request_path} HTTP/1.1\r\n\
         Host: {host}\r\n"
    );
    for h in extra_headers {
        header_block.push_str(h);
        header_block.push_str("\r\n");
    }
    header_block.push_str(&format!(
        "Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    ));

    if is_https {
        let connector =
            native_tls::TlsConnector::new().map_err(|e| format!("TLS error: {e}"))?;
        let stream =
            TcpStream::connect((host, port)).map_err(|e| format!("connection error: {e}"))?;
        let mut tls_stream = connector
            .connect(host, stream)
            .map_err(|e| format!("TLS handshake error: {e}"))?;
        tls_stream
            .write_all(header_block.as_bytes())
            .map_err(|e| format!("write error: {e}"))?;
        tls_stream
            .flush()
            .map_err(|e| format!("flush error: {e}"))?;
        read_http_response(BufReader::new(tls_stream))
    } else {
        let mut stream =
            TcpStream::connect((host, port)).map_err(|e| format!("connection error: {e}"))?;
        stream
            .write_all(header_block.as_bytes())
            .map_err(|e| format!("write error: {e}"))?;
        stream.flush().map_err(|e| format!("flush error: {e}"))?;
        read_http_response(BufReader::new(stream))
    }
}

fn read_http_response<R: BufRead>(mut reader: R) -> Result<String, String> {
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .map_err(|e| e.to_string())?;

    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_lowercase();
        if lower.starts_with("content-length:") {
            content_length = lower.split(':').nth(1).and_then(|v| v.trim().parse().ok());
        }
        if lower.contains("transfer-encoding: chunked") {
            chunked = true;
        }
    }

    let body = if let Some(len) = content_length {
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
        String::from_utf8(buf).map_err(|e| e.to_string())?
    } else if chunked {
        read_chunked_body(&mut reader)?
    } else {
        let mut buf = String::new();
        reader.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        buf
    };

    if !status_line.contains("200") {
        return Err(format!("HTTP error: {}\n{}", status_line.trim(), body));
    }

    Ok(body)
}

fn read_chunked_body<R: BufRead>(reader: &mut R) -> Result<String, String> {
    let mut body = String::new();
    loop {
        let mut size_line = String::new();
        reader
            .read_line(&mut size_line)
            .map_err(|e| e.to_string())?;
        let size = usize::from_str_radix(size_line.trim(), 16)
            .map_err(|e| format!("invalid chunk size: {e}"))?;
        if size == 0 {
            break;
        }
        let mut chunk = vec![0u8; size];
        reader.read_exact(&mut chunk).map_err(|e| e.to_string())?;
        body.push_str(&String::from_utf8(chunk).map_err(|e| e.to_string())?);
        let mut crlf = String::new();
        reader.read_line(&mut crlf).map_err(|e| e.to_string())?;
    }
    Ok(body)
}

/// Appends a labelled raw log entry to a file.
pub fn log_raw(log_path: &Option<PathBuf>, label: &str, data: &str) {
    if let Some(ref path) = log_path {
        use std::fs::OpenOptions;
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
            let _ = writeln!(f, "\n=== {label} [{ts}] ===\n{data}");
        }
    }
}
