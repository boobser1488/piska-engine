//! Крошечный статический сервер для проверки веб-сборки: только `127.0.0.1`,
//! только GET/HEAD, правильные MIME-типы (`application/wasm` обязателен для
//! потоковой компиляции wasm), без кэша — после пересборки браузер берёт новые файлы.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

use crate::{Error, Result};

pub fn mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Путь запроса → файл внутри `root` (без `..`, без абсолютных путей).
pub fn resolve(root: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next().unwrap_or("/");
    let decoded = percent_decode(path)?;
    let mut file = root.to_path_buf();
    for component in Path::new(decoded.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part) => file.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if file.is_dir() {
        file.push("index.html");
    }
    Some(file)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn respond(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // Заголовки запроса не нужны, но их надо дочитать.
    let mut line = String::new();
    while reader.read_line(&mut line)? > 2 {
        line.clear();
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");

    let send = |stream: &mut TcpStream, status: &str, mime: &str, body: &[u8], head: bool| {
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\
             Cache-Control: no-cache\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        if !head {
            stream.write_all(body)?;
        }
        stream.flush()
    };
    if method != "GET" && method != "HEAD" {
        return send(&mut stream, "405 Method Not Allowed", "text/plain", b"only GET and HEAD", false);
    }
    let head = method == "HEAD";
    match resolve(root, target).filter(|file| file.is_file()) {
        Some(file) => match std::fs::read(&file) {
            Ok(body) => {
                println!("200 {target}");
                send(&mut stream, "200 OK", mime_type(&file), &body, head)
            }
            Err(_) => send(&mut stream, "500 Internal Server Error", "text/plain", b"cannot read the file", head),
        },
        None => {
            println!("404 {target}");
            send(&mut stream, "404 Not Found", "text/plain", b"not found", head)
        }
    }
}

/// Раздавать `dir` на `http://127.0.0.1:<port>/`, пока процесс не остановят.
pub fn serve(dir: &Path, port: u16, open: bool) -> Result<()> {
    if !dir.join("index.html").is_file() {
        return Err(Error(format!(
            "`{}` has no index.html; export first: cargo run -p piska-export -- web --game <name>",
            dir.display()
        )));
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| {
        Error(format!("cannot listen on 127.0.0.1:{port}: {e}. Is the port busy? Try --port {}", port + 1))
    })?;
    let url = format!("http://127.0.0.1:{port}/");
    println!("piska-export: serving {} at {url} (Ctrl+C to stop)", dir.display());
    if open {
        open_browser(&url);
    }
    let root = dir.to_path_buf();
    for stream in listener.incoming().flatten() {
        let root = root.clone();
        std::thread::spawn(move || {
            let _ = respond(stream, &root);
        });
    }
    Ok(())
}

fn open_browser(url: &str) {
    let result = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(err) = result {
        println!("piska-export: open {url} in a browser ({err})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_stay_inside_the_root() {
        let root = Path::new("site");
        assert_eq!(resolve(root, "/game.js?v=2"), Some(root.join("game.js")));
        assert_eq!(resolve(root, "/a%20b.png"), Some(root.join("a b.png")));
        assert_eq!(resolve(root, "/../secret.txt"), None);
        assert_eq!(resolve(root, "/x/%2e%2e/%2e%2e/secret"), None);
        assert_eq!(mime_type(Path::new("game_bg.wasm")), "application/wasm");
        assert_eq!(mime_type(Path::new("index.html")), "text/html; charset=utf-8");
    }
}
