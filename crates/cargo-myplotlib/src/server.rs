use std::fs::{self, File};
use std::io;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};

use percent_encoding::percent_decode_str;
use tiny_http::{Header, Method, Request, Response, ResponseBox, Server, StatusCode};

use crate::{Error, Serve};

const INDEX_FILE: &str = "index.html";

pub(crate) fn serve(config: Serve) -> Result<(), Error> {
    let working_directory = std::env::current_dir()
        .map_err(|error| Error::new(format!("failed to read the working directory: {error}")))?;
    let requested_root = if config.dir.is_absolute() {
        config.dir
    } else {
        working_directory.join(config.dir)
    };
    let root = fs::canonicalize(&requested_root).map_err(|error| {
        Error::new(format!(
            "failed to open web root {}: {error}",
            requested_root.display()
        ))
    })?;
    if !root.is_dir() {
        return Err(Error::new(format!(
            "web root is not a directory: {}",
            root.display()
        )));
    }

    let requested_address = SocketAddr::new(config.host, config.port);
    let server = Server::http(requested_address).map_err(|error| {
        Error::new(format!(
            "failed to bind development server to {requested_address}: {error}"
        ))
    })?;
    let address = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| Error::new("development server did not bind an IP socket"))?;

    println!("Serving {} at http://{address}/", root.display());
    println!("Cross-origin isolation enabled: COOP same-origin, COEP require-corp");

    for request in server.incoming_requests() {
        let method = request.method().clone();
        let url = request.url().to_owned();
        if let Err(error) = serve_request(request, &root) {
            eprintln!("failed to respond to {method} {url}: {error}");
        }
    }

    Ok(())
}

fn serve_request(request: Request, root: &Path) -> io::Result<()> {
    let response = response_for(request.method(), request.url(), root);
    request.respond(response)
}

fn response_for(method: &Method, url: &str, root: &Path) -> ResponseBox {
    if !matches!(method, Method::Get | Method::Head) {
        let mut response = text_response(StatusCode(405), "Method not allowed\n");
        response.add_header(header("Allow", "GET, HEAD"));
        return response;
    }

    let path = match resolve_path(root, url) {
        Ok(path) => path,
        Err(RouteError::BadRequest) => {
            return text_response(StatusCode(400), "Bad request\n");
        }
        Err(RouteError::NotFound | RouteError::OutsideRoot) => {
            return text_response(StatusCode(404), "Not found\n");
        }
        Err(RouteError::Io(error)) => {
            eprintln!("failed to resolve {url}: {error}");
            return text_response(StatusCode(500), "Internal server error\n");
        }
    };

    match File::open(&path) {
        Ok(file) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            with_common_headers(
                Response::from_file(file).with_header(header("Content-Type", mime.essence_str())),
                cache_control(url),
            )
            .boxed()
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            text_response(StatusCode(404), "Not found\n")
        }
        Err(error) => {
            eprintln!("failed to open {}: {error}", path.display());
            text_response(StatusCode(500), "Internal server error\n")
        }
    }
}

fn text_response(status: StatusCode, body: &str) -> ResponseBox {
    with_common_headers(
        Response::from_string(body)
            .with_status_code(status)
            .with_header(header("Content-Type", "text/plain; charset=utf-8")),
        "no-store",
    )
    .boxed()
}

fn with_common_headers<R: io::Read>(mut response: Response<R>, cache_control: &str) -> Response<R> {
    for (name, value) in [
        ("Cross-Origin-Opener-Policy", "same-origin"),
        ("Cross-Origin-Embedder-Policy", "require-corp"),
        ("Cross-Origin-Resource-Policy", "same-origin"),
        ("Cache-Control", cache_control),
    ] {
        response.add_header(header(name, value));
    }
    response
}

fn cache_control(url: &str) -> &'static str {
    let versioned = url.split_once('?').is_some_and(|(path, query)| {
        is_generated_wasm_asset(path)
            && query
                .split('&')
                .filter_map(|field| field.split_once('='))
                .any(|(name, value)| name == "v" && is_content_version(value))
    });
    if versioned {
        "public, max-age=31536000, immutable"
    } else {
        "no-store"
    }
}

fn is_generated_wasm_asset(path: &str) -> bool {
    matches!(
        path.rsplit('/').next(),
        Some("bindings.js" | "bindings_bg.wasm" | "workerHelpers.no-bundler.js")
    )
}

fn is_content_version(value: &str) -> bool {
    value.len() == 16 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("static and MIME headers must contain ASCII")
}

#[derive(Debug)]
enum RouteError {
    BadRequest,
    NotFound,
    OutsideRoot,
    Io(io::Error),
}

fn resolve_path(root: &Path, url: &str) -> Result<PathBuf, RouteError> {
    let encoded_path = url.split_once('?').map_or(url, |(path, _)| path);
    if !encoded_path.starts_with('/') || has_invalid_percent_encoding(encoded_path) {
        return Err(RouteError::BadRequest);
    }
    let decoded = percent_decode_str(encoded_path)
        .decode_utf8()
        .map_err(|_| RouteError::BadRequest)?;
    if decoded.contains(['\0', '\\']) {
        return Err(RouteError::BadRequest);
    }

    let mut relative = PathBuf::new();
    for component in Path::new(decoded.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(RouteError::OutsideRoot);
            }
        }
    }

    let mut candidate = root.join(relative);
    match fs::metadata(&candidate) {
        Ok(metadata) if metadata.is_dir() => candidate.push(INDEX_FILE),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(RouteError::NotFound);
        }
        Err(error) => return Err(RouteError::Io(error)),
    }

    let canonical = fs::canonicalize(&candidate).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            RouteError::NotFound
        } else {
            RouteError::Io(error)
        }
    })?;
    if !canonical.starts_with(root) {
        return Err(RouteError::OutsideRoot);
    }
    if !canonical.is_file() {
        return Err(RouteError::NotFound);
    }
    Ok(canonical)
}

fn has_invalid_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return true;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cargo-myplotlib-server-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        fs::canonicalize(path).unwrap()
    }

    fn response(method: Method, url: &str, root: &Path) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let response = response_for(&method, url, root);
        let status = response.status_code().0;
        let headers = response
            .headers()
            .iter()
            .map(|header| {
                (
                    header.field.as_str().to_string(),
                    header.value.as_str().to_string(),
                )
            })
            .collect();
        let mut body = Vec::new();
        response.into_reader().read_to_end(&mut body).unwrap();
        (status, headers, body)
    }

    fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn serves_index_assets_and_wasm_with_expected_types() {
        let root = temporary_directory();
        fs::write(root.join("index.html"), "<h1>Myplotlib</h1>").unwrap();
        fs::write(root.join("app.js"), "export const ready = true;").unwrap();
        fs::write(root.join("app.wasm"), b"\0asm").unwrap();

        let (status, headers, body) = response(Method::Get, "/?cache=bust", &root);
        assert_eq!(status, 200);
        assert_eq!(body, b"<h1>Myplotlib</h1>");
        assert_eq!(header_value(&headers, "Content-Type"), Some("text/html"));

        let (_, headers, _) = response(Method::Get, "/app.js", &root);
        assert_eq!(
            header_value(&headers, "Content-Type"),
            Some("text/javascript")
        );

        let (_, headers, body) = response(Method::Get, "/app.wasm", &root);
        assert_eq!(body, b"\0asm");
        assert_eq!(
            header_value(&headers, "Content-Type"),
            Some("application/wasm")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adds_cross_origin_isolation_headers_to_successes_and_errors() {
        let root = temporary_directory();
        fs::write(root.join("index.html"), "ok").unwrap();

        for (method, url, expected_status) in [
            (Method::Get, "/", 200),
            (Method::Head, "/", 200),
            (Method::Get, "/missing", 404),
            (Method::Post, "/", 405),
        ] {
            let (status, headers, _) = response(method, url, &root);
            assert_eq!(status, expected_status);
            assert_eq!(
                header_value(&headers, "Cross-Origin-Opener-Policy"),
                Some("same-origin")
            );
            assert_eq!(
                header_value(&headers, "Cross-Origin-Embedder-Policy"),
                Some("require-corp")
            );
            assert_eq!(
                header_value(&headers, "Cross-Origin-Resource-Policy"),
                Some("same-origin")
            );
            assert_eq!(header_value(&headers, "Cache-Control"), Some("no-store"));
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn caches_only_versioned_assets() {
        let root = temporary_directory();
        fs::write(root.join("app.js"), "ok").unwrap();
        fs::write(root.join("bindings.js"), "ok").unwrap();

        for url in [
            "/app.js",
            "/app.js?v=0123456789abcdef",
            "/bindings.js?v=1234",
            "/bindings.js?v=0123456789abcdeg",
        ] {
            let (_, headers, _) = response(Method::Get, url, &root);
            assert_eq!(header_value(&headers, "Cache-Control"), Some("no-store"));
        }

        for url in [
            "/bindings.js?v=0123456789abcdef",
            "/bindings_bg.wasm?v=0123456789abcdef",
            "/snippets/rayon/src/workerHelpers.no-bundler.js?v=0123456789abcdef",
        ] {
            assert_eq!(cache_control(url), "public, max-age=31536000, immutable");
        }

        let (_, headers, _) = response(Method::Get, "/bindings.js?v=0123456789abcdef", &root);
        assert_eq!(
            header_value(&headers, "Cache-Control"),
            Some("public, max-age=31536000, immutable")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_and_traversing_paths() {
        let root = temporary_directory();
        fs::write(root.join("index.html"), "ok").unwrap();

        for url in [
            "relative",
            "/bad%2",
            "/%FF",
            "/../index.html",
            "/%2e%2e/index.html",
        ] {
            let (status, _, _) = response(Method::Get, url, &root);
            assert!(
                matches!(status, 400 | 404),
                "unexpected status for {url}: {status}"
            );
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_that_escape_the_web_root() {
        use std::os::unix::fs::symlink;

        let root = temporary_directory();
        let outside = temporary_directory();
        fs::write(outside.join("secret.txt"), "secret").unwrap();
        symlink(outside.join("secret.txt"), root.join("secret.txt")).unwrap();

        let (status, _, body) = response(Method::Get, "/secret.txt", &root);
        assert_eq!(status, 404);
        assert_ne!(body, b"secret");

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
